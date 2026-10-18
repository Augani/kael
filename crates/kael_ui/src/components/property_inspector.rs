//! Typed, grouped property editing with validation and bounded undo/redo.
use super::{
    button::{Button, ButtonSize, ButtonVariant},
    checkbox::Checkbox,
    dropdown::{Dropdown, DropdownItem, DropdownState},
    input::{Input, InputState},
    number_input::{NumberInput, NumberInputState},
};
use crate::theme::Theme;
use kael::{prelude::*, *};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

/// A property value; types must agree with their declared editor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PropertyValue {
    Text(String),
    Number(f64),
    Boolean(bool),
    Choice(String),
}
/// A choice's stable value and user-visible label.
#[derive(Clone, Debug)]
pub struct PropertyChoice {
    pub value: String,
    pub label: SharedString,
}
impl PropertyChoice {
    pub fn new(value: impl Into<String>, label: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
        }
    }
}
/// Editor and validation contract for a property.
#[derive(Clone, Debug)]
pub enum PropertyKind {
    Text {
        max_chars: Option<usize>,
    },
    Number {
        min: Option<f64>,
        max: Option<f64>,
        step: f64,
        precision: usize,
    },
    Boolean,
    Choice {
        options: Vec<PropertyChoice>,
    },
}
#[derive(Clone, Debug)]
pub struct PropertyField {
    pub id: String,
    pub label: SharedString,
    pub description: Option<SharedString>,
    pub kind: PropertyKind,
    pub value: PropertyValue,
    pub read_only: bool,
}
impl PropertyField {
    pub fn new(
        id: impl Into<String>,
        label: impl Into<SharedString>,
        kind: PropertyKind,
        value: PropertyValue,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: None,
            kind,
            value,
            read_only: false,
        }
    }
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }
}
#[derive(Clone, Debug)]
pub struct PropertyGroup {
    pub id: String,
    pub title: SharedString,
    pub fields: Vec<PropertyField>,
}
impl PropertyGroup {
    pub fn new(
        id: impl Into<String>,
        title: impl Into<SharedString>,
        fields: Vec<PropertyField>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            fields,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct PropertyChange {
    pub id: String,
    pub before: PropertyValue,
    pub after: PropertyValue,
}
#[derive(Clone, Debug, PartialEq)]
pub enum PropertyInspectorEvent {
    Changed(Vec<PropertyChange>),
    Rejected { id: String, message: SharedString },
}
#[derive(Clone)]
enum PropertyEditor {
    Text(Entity<InputState>),
    Number(Entity<NumberInputState>),
    Choice(Entity<DropdownState>),
    Boolean,
}
/// Persistent editor state. Groups/fields are validated at construction; history
/// retains at most `history_limit` transactions (128 by default). `set_values`
/// validates an entire batch before modifying any property.
pub struct PropertyInspectorState {
    groups: Vec<PropertyGroup>,
    indices: HashMap<String, (usize, usize)>,
    editors: HashMap<String, PropertyEditor>,
    errors: HashMap<String, SharedString>,
    collapsed: HashSet<String>,
    undo: VecDeque<Vec<PropertyChange>>,
    redo: Vec<Vec<PropertyChange>>,
    history_limit: usize,
    dirty: HashSet<String>,
}
impl EventEmitter<PropertyInspectorEvent> for PropertyInspectorState {}
fn validate_property(field: &PropertyField, value: &PropertyValue) -> Result<(), String> {
    match (&field.kind, value) {
        (PropertyKind::Text { max_chars }, PropertyValue::Text(value)) => {
            if max_chars.is_some_and(|limit| value.chars().count() > limit) {
                return Err(format!("Maximum {} characters", max_chars.unwrap()));
            }
        }
        (
            PropertyKind::Number {
                min,
                max,
                step,
                precision,
            },
            PropertyValue::Number(value),
        ) => {
            if !value.is_finite()
                || !step.is_finite()
                || *step <= 0.0
                || *precision > 12
                || min.is_some_and(|min| !min.is_finite())
                || max.is_some_and(|max| !max.is_finite())
                || matches!((min, max), (Some(min), Some(max)) if min > max)
            {
                return Err("Invalid numeric value or constraints".into());
            }
            if min.is_some_and(|min| *value < min) || max.is_some_and(|max| *value > max) {
                return Err("Value is outside the allowed range".into());
            }
        }
        (PropertyKind::Boolean, PropertyValue::Boolean(_)) => {}
        (PropertyKind::Choice { options }, PropertyValue::Choice(value)) => {
            let mut seen = HashSet::new();
            if options.is_empty()
                || options.iter().any(|option| !seen.insert(&option.value))
                || !options.iter().any(|option| &option.value == value)
            {
                return Err("Invalid or duplicate choice".into());
            }
        }
        _ => return Err("Property value has the wrong type".into()),
    }
    Ok(())
}
impl PropertyInspectorState {
    pub fn new(groups: Vec<PropertyGroup>, cx: &mut Context<Self>) -> Result<Self, String> {
        let mut indices = HashMap::new();
        let mut group_ids = HashSet::new();
        let mut editors = HashMap::new();
        for (group_index, group) in groups.iter().enumerate() {
            if group.id.is_empty() || !group_ids.insert(&group.id) {
                return Err("group IDs must be nonempty and unique".into());
            }
            for (field_index, field) in group.fields.iter().enumerate() {
                if field.id.is_empty()
                    || indices
                        .insert(field.id.clone(), (group_index, field_index))
                        .is_some()
                {
                    return Err("property IDs must be nonempty and unique".into());
                }
                validate_property(field, &field.value)?;
                let editor = match (&field.kind, &field.value) {
                    (PropertyKind::Text { .. }, PropertyValue::Text(value)) => {
                        PropertyEditor::Text(cx.new(|cx| {
                            let mut state = InputState::new(cx);
                            state.content = value.clone().into();
                            state
                        }))
                    }
                    (
                        PropertyKind::Number {
                            min,
                            max,
                            step,
                            precision,
                        },
                        PropertyValue::Number(value),
                    ) => PropertyEditor::Number(cx.new(|cx| {
                        let mut state = NumberInputState::with_value(cx, *value);
                        state.set_min(*min, cx);
                        state.set_max(*max, cx);
                        state.set_step(*step, cx);
                        state.set_precision(*precision, cx);
                        state
                    })),
                    (PropertyKind::Choice { .. }, _) => {
                        PropertyEditor::Choice(cx.new(DropdownState::new))
                    }
                    _ => PropertyEditor::Boolean,
                };
                editors.insert(field.id.clone(), editor);
            }
        }
        Ok(Self {
            groups,
            indices,
            editors,
            errors: HashMap::new(),
            collapsed: HashSet::new(),
            undo: VecDeque::new(),
            redo: Vec::new(),
            history_limit: 128,
            dirty: HashSet::new(),
        })
    }
    pub fn groups(&self) -> &[PropertyGroup] {
        &self.groups
    }
    pub fn value(&self, id: &str) -> Option<&PropertyValue> {
        let (group, field) = *self.indices.get(id)?;
        Some(&self.groups[group].fields[field].value)
    }
    pub fn error(&self, id: &str) -> Option<&str> {
        self.errors.get(id).map(AsRef::as_ref)
    }
    pub fn values(&self) -> HashMap<String, PropertyValue> {
        self.indices
            .keys()
            .map(|id| (id.clone(), self.value(id).unwrap().clone()))
            .collect()
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn set_history_limit(&mut self, limit: usize) {
        self.history_limit = limit;
        while self.undo.len() > limit {
            self.undo.pop_front();
        }
        if self.redo.len() > limit {
            self.redo.drain(0..self.redo.len() - limit);
        }
    }
    pub fn set_value(
        &mut self,
        id: &str,
        value: PropertyValue,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        self.set_values(vec![(id.to_string(), value)], cx)
    }
    pub fn set_values(
        &mut self,
        values: Vec<(String, PropertyValue)>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let mut changes = Vec::new();
        let mut seen = HashSet::new();
        let mut validated = Vec::new();
        for (id, value) in values {
            let result = if !seen.insert(id.clone()) {
                Err("Duplicate property in transaction".into())
            } else if let Some(&(group, field)) = self.indices.get(&id) {
                let field = &self.groups[group].fields[field];
                if field.read_only {
                    Err("Property is read only".into())
                } else {
                    validate_property(field, &value)
                }
            } else {
                Err("Unknown property".into())
            };
            if let Err(message) = result {
                if self.indices.contains_key(&id) {
                    self.errors.insert(id.clone(), message.clone().into());
                }
                cx.emit(PropertyInspectorEvent::Rejected {
                    id,
                    message: message.clone().into(),
                });
                cx.notify();
                return Err(message);
            }
            validated.push(id.clone());
            let before = self.value(&id).unwrap().clone();
            if before != value {
                changes.push(PropertyChange {
                    id,
                    before,
                    after: value,
                });
            }
        }
        let cleared = validated
            .iter()
            .filter(|id| self.errors.remove(*id).is_some())
            .count();
        if changes.is_empty() {
            if cleared > 0 {
                cx.notify();
            }
            return Ok(());
        }
        self.apply_changes(&changes, false);
        self.redo.clear();
        if self.history_limit > 0 {
            self.undo.push_back(changes.clone());
            while self.undo.len() > self.history_limit {
                self.undo.pop_front();
            }
        }
        cx.emit(PropertyInspectorEvent::Changed(changes));
        cx.notify();
        Ok(())
    }
    fn apply_changes(&mut self, changes: &[PropertyChange], reverse: bool) {
        for change in changes {
            let (group, field) = self.indices[&change.id];
            self.groups[group].fields[field].value = if reverse {
                change.before.clone()
            } else {
                change.after.clone()
            };
            self.errors.remove(&change.id);
            self.dirty.insert(change.id.clone());
        }
    }
    pub fn undo(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(changes) = self.undo.pop_back() else {
            return false;
        };
        self.apply_changes(&changes, true);
        cx.emit(PropertyInspectorEvent::Changed(
            changes
                .iter()
                .map(|change| PropertyChange {
                    id: change.id.clone(),
                    before: change.after.clone(),
                    after: change.before.clone(),
                })
                .collect(),
        ));
        self.redo.push(changes);
        cx.notify();
        true
    }
    pub fn redo(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(changes) = self.redo.pop() else {
            return false;
        };
        self.apply_changes(&changes, false);
        self.undo.push_back(changes.clone());
        cx.emit(PropertyInspectorEvent::Changed(changes));
        cx.notify();
        true
    }
    pub fn toggle_group(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.groups.iter().any(|group| group.id == id) {
            return;
        }
        if !self.collapsed.remove(id) {
            self.collapsed.insert(id.to_string());
        }
        cx.notify();
    }
    fn sync_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for id in self.dirty.drain().collect::<Vec<_>>() {
            match (&self.editors[&id], self.value(&id).unwrap()) {
                (PropertyEditor::Text(editor), PropertyValue::Text(value)) => {
                    editor.update(cx, |state, cx| {
                        if state.content() != value {
                            state.set_value(value.clone(), window, cx);
                        }
                    })
                }
                (PropertyEditor::Number(editor), PropertyValue::Number(value)) => {
                    editor.update(cx, |state, cx| state.set_value(*value, cx))
                }
                _ => {}
            }
        }
    }
}
/// A structured editor built on existing themed input components. Groups can
/// collapse, read-only fields remain visible, invalid edits report inline errors,
/// and Undo/Redo controls apply whole transactions.
#[derive(IntoElement)]
pub struct PropertyInspector {
    id: ElementId,
    state: Entity<PropertyInspectorState>,
    title: SharedString,
    style: StyleRefinement,
}
impl PropertyInspector {
    pub fn new(id: impl Into<ElementId>, state: Entity<PropertyInspectorState>) -> Self {
        Self {
            id: id.into(),
            state,
            title: "Properties".into(),
            style: StyleRefinement::default(),
        }
    }
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = title.into();
        self
    }
}
impl Styled for PropertyInspector {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
impl RenderOnce for PropertyInspector {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        super::model_observer::observe_model(self.id.clone(), &self.state, window, cx);
        self.state
            .update(cx, |state, cx| state.sync_editors(window, cx));
        let undo = self.state.downgrade();
        let redo = self.state.downgrade();
        let key = self.state.downgrade();
        let border = Theme::of(cx).tokens.border;
        let foreground = Theme::of(cx).tokens.foreground;
        let muted = Theme::of(cx).tokens.muted_foreground;
        let mut root = div()
            .id(self.id)
            .size_full()
            .flex()
            .flex_col()
            .min_h(px(0.0))
            .bg(Theme::of(cx).tokens.background)
            .text_color(foreground)
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group)
                    .label(self.title.to_string()),
            )
            .on_key_down(move |event, _, cx| {
                if (event.keystroke.modifiers.control || event.keystroke.modifiers.platform)
                    && event.keystroke.key == "z"
                {
                    let _ = key.update(cx, |state, cx| {
                        if event.keystroke.modifiers.shift {
                            state.redo(cx)
                        } else {
                            state.undo(cx)
                        }
                    });
                    cx.stop_propagation();
                }
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .p_3()
                    .border_b_1()
                    .border_color(border)
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(self.title))
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(
                                Button::new("property-undo", "Undo")
                                    .size(ButtonSize::Sm)
                                    .variant(ButtonVariant::Ghost)
                                    .disabled(!self.state.read(cx).can_undo())
                                    .on_click(move |_, _, cx| {
                                        let _ = undo.update(cx, |state, cx| state.undo(cx));
                                    }),
                            )
                            .child(
                                Button::new("property-redo", "Redo")
                                    .size(ButtonSize::Sm)
                                    .variant(ButtonVariant::Ghost)
                                    .disabled(!self.state.read(cx).can_redo())
                                    .on_click(move |_, _, cx| {
                                        let _ = redo.update(cx, |state, cx| state.redo(cx));
                                    }),
                            ),
                    ),
            );
        let mut body = div()
            .id("property-scroll")
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .p_3()
            .flex()
            .flex_col()
            .gap_3();
        for group in &self.state.read(cx).groups {
            let collapsed = self.state.read(cx).collapsed.contains(&group.id);
            let collapse = self.state.downgrade();
            let group_id = group.id.clone();
            let mut section = div().flex().flex_col().gap_2().child(
                Button::new(
                    (ElementId::from("property-group"), group.id.clone()),
                    format!("{} {}", if collapsed { "▸" } else { "▾" }, group.title),
                )
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Sm)
                .on_click(move |_, _, cx| {
                    let _ = collapse.update(cx, |state, cx| state.toggle_group(&group_id, cx));
                }),
            );
            if !collapsed {
                for field in &group.fields {
                    let edit = self.state.downgrade();
                    let id = field.id.clone();
                    let editor = self.state.read(cx).editors[&id].clone();
                    let control = match (&field.kind, &field.value, editor) {
                        (PropertyKind::Text { .. }, _, PropertyEditor::Text(editor)) => {
                            Input::new(&editor)
                                .label(field.label.clone())
                                .read_only(field.read_only)
                                .on_change(move |value, cx| {
                                    let _ = edit.update(cx, |state, cx| {
                                        state.set_value(
                                            &id,
                                            PropertyValue::Text(value.to_string()),
                                            cx,
                                        )
                                    });
                                })
                                .into_any_element()
                        }
                        (PropertyKind::Number { .. }, _, PropertyEditor::Number(editor)) => {
                            NumberInput::new(editor)
                                .label(field.label.clone())
                                .read_only(field.read_only)
                                .show_buttons(true)
                                .on_change(move |value, _, cx| {
                                    let _ = edit.update(cx, |state, cx| {
                                        state.set_value(&id, PropertyValue::Number(value), cx)
                                    });
                                })
                                .into_any_element()
                        }
                        (PropertyKind::Boolean, PropertyValue::Boolean(value), _) => {
                            Checkbox::new((ElementId::from("property-boolean"), id.clone()))
                                .label(field.label.clone())
                                .checked(*value)
                                .disabled(field.read_only)
                                .on_click(move |value, _, cx| {
                                    let _ = edit.update(cx, |state, cx| {
                                        state.set_value(&id, PropertyValue::Boolean(*value), cx)
                                    });
                                })
                                .into_any_element()
                        }
                        (
                            PropertyKind::Choice { options },
                            PropertyValue::Choice(value),
                            PropertyEditor::Choice(editor),
                        ) if !field.read_only => {
                            let label = options
                                .iter()
                                .find(|option| &option.value == value)
                                .unwrap()
                                .label
                                .clone();
                            let items = options
                                .iter()
                                .map(|option| {
                                    let edit = edit.clone();
                                    let id = id.clone();
                                    let value = option.value.clone();
                                    DropdownItem::new(option.value.clone(), option.label.clone())
                                        .on_click(move |_, cx| {
                                            let _ = edit.update(cx, |state, cx| {
                                                state.set_value(
                                                    &id,
                                                    PropertyValue::Choice(value.clone()),
                                                    cx,
                                                )
                                            });
                                        })
                                })
                                .collect();
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(div().text_sm().child(field.label.clone()))
                                .child(
                                    Dropdown::new(
                                        editor,
                                        Button::new(
                                            (ElementId::from("property-choice"), field.id.clone()),
                                            label,
                                        )
                                        .variant(ButtonVariant::Outline),
                                    )
                                    .accessibility_label(format!("{} choices", field.label))
                                    .items(items),
                                )
                                .into_any_element()
                        }
                        _ => div()
                            .text_sm()
                            .child(format!("{}: {:?}", field.label, field.value))
                            .into_any_element(),
                    };
                    let mut row = div().flex().flex_col().gap_1().child(control);
                    if let Some(description) = field.description.as_ref() {
                        row =
                            row.child(div().text_xs().text_color(muted).child(description.clone()));
                    }
                    if let Some(error) = self.state.read(cx).errors.get(&field.id) {
                        row = row.child(
                            div()
                                .text_xs()
                                .text_color(Theme::of(cx).tokens.destructive)
                                .child(error.clone()),
                        );
                    }
                    section = section.child(row);
                }
            }
            body = body.child(section);
        }
        root = root.child(body);
        root.map(|mut element| {
            element.style().refine(&self.style);
            element
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn groups() -> Vec<PropertyGroup> {
        vec![PropertyGroup::new(
            "appearance",
            "Appearance",
            vec![
                PropertyField::new(
                    "name",
                    "Name",
                    PropertyKind::Text {
                        max_chars: Some(20),
                    },
                    PropertyValue::Text("Document".into()),
                ),
                PropertyField::new(
                    "opacity",
                    "Opacity",
                    PropertyKind::Number {
                        min: Some(0.0),
                        max: Some(1.0),
                        step: 0.1,
                        precision: 2,
                    },
                    PropertyValue::Number(1.0),
                ),
                PropertyField::new(
                    "visible",
                    "Visible",
                    PropertyKind::Boolean,
                    PropertyValue::Boolean(true),
                ),
                PropertyField::new(
                    "mode",
                    "Mode",
                    PropertyKind::Choice {
                        options: vec![
                            PropertyChoice::new("light", "Light"),
                            PropertyChoice::new("dark", "Dark"),
                        ],
                    },
                    PropertyValue::Choice("light".into()),
                ),
            ],
        )]
    }
    #[::core::prelude::v1::test]
    fn unknown_field_rejections_do_not_accumulate_inline_errors_or_history() {
        let mut cx = TestAppContext::single();
        let state = cx.new(|cx| PropertyInspectorState::new(groups(), cx).unwrap());
        state.update(&mut cx, |state, cx| {
            for index in 0..10_000 {
                assert!(
                    state
                        .set_value(
                            &format!("unknown-{index}"),
                            PropertyValue::Boolean(true),
                            cx
                        )
                        .is_err()
                );
            }
            assert!(state.errors.is_empty());
            assert!(!state.can_undo());
            assert!(!state.can_redo());
            assert_eq!(state.value("visible"), Some(&PropertyValue::Boolean(true)));
        });
    }
    #[::core::prelude::v1::test]
    fn typed_batches_validate_atomically_and_undo_redo_are_bounded() {
        let mut cx = TestAppContext::single();
        let state = cx.new(|cx| PropertyInspectorState::new(groups(), cx).unwrap());
        state.update(&mut cx, |state, cx| {
            assert!(
                state
                    .set_values(
                        vec![
                            ("name".into(), PropertyValue::Text("Changed".into())),
                            ("opacity".into(), PropertyValue::Number(2.0))
                        ],
                        cx
                    )
                    .is_err()
            );
            assert_eq!(
                state.value("name"),
                Some(&PropertyValue::Text("Document".into()))
            );
            assert!(!state.can_undo());
            assert!(state.error("opacity").is_some());
            state
                .set_values(
                    vec![
                        ("name".into(), PropertyValue::Text("Changed".into())),
                        ("opacity".into(), PropertyValue::Number(0.5)),
                    ],
                    cx,
                )
                .unwrap();
            assert!(state.undo(cx));
            assert_eq!(
                state.value("name"),
                Some(&PropertyValue::Text("Document".into()))
            );
            assert_eq!(state.value("opacity"), Some(&PropertyValue::Number(1.0)));
            assert!(state.redo(cx));
            assert_eq!(
                state.value("name"),
                Some(&PropertyValue::Text("Changed".into()))
            );
            state.set_history_limit(2);
            for value in [0.4, 0.3, 0.2, 0.1] {
                state
                    .set_value("opacity", PropertyValue::Number(value), cx)
                    .unwrap();
            }
            assert_eq!(state.undo.len(), 2);
            assert!(state.undo(cx));
            assert!(state.undo(cx));
            assert!(!state.undo(cx));
            assert!(
                state
                    .set_value("mode", PropertyValue::Choice("unknown".into()), cx)
                    .is_err()
            );
            assert!(
                state
                    .set_value("opacity", PropertyValue::Number(f64::NAN), cx)
                    .is_err()
            );
            assert!(
                state
                    .set_value("visible", PropertyValue::Text("yes".into()), cx)
                    .is_err()
            );
        });
    }
    struct Host {
        state: Entity<PropertyInspectorState>,
    }
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            PropertyInspector::new("property-test", self.state.clone())
                .w(px(440.0))
                .h(px(600.0))
        }
    }
    #[::core::prelude::v1::test]
    fn rendered_text_caret_numeric_edit_and_history_stay_synchronized() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| {
            crate::init(cx);
            crate::theme::install_theme(cx, Theme::dark());
        });
        let state = cx.new(|cx| PropertyInspectorState::new(groups(), cx).unwrap());
        let (_host, window) = cx.add_window_view({
            let state = state.clone();
            move |_, _| Host { state }
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
            let PropertyEditor::Text(editor) = &state.read(cx).editors["name"] else {
                panic!()
            };
            window.focus(&editor.focus_handle(cx));
        });
        #[cfg(target_os = "macos")]
        window.simulate_keystrokes("cmd-a");
        #[cfg(not(target_os = "macos"))]
        window.simulate_keystrokes("ctrl-a");
        window.simulate_input("Renamed🙂");
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        window.simulate_keystrokes("left");
        window.simulate_input("X");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(
                state.read(cx).value("name"),
                Some(&PropertyValue::Text("RenamedX🙂".into()))
            );
            let number = window
                .accessibility_tree()
                .nodes
                .values()
                .find(|node| {
                    node.label.as_deref() == Some("Opacity")
                        && node.actions.contains(&AccessibilityAction::SetValue)
                })
                .unwrap()
                .id;
            window.dispatch_accessibility_action_for_test(
                AccessibilityActionRequest::with_payload(
                    number,
                    AccessibilityAction::SetValue,
                    AccessibilityActionPayload::NumericValue(0.4),
                ),
            );
        });
        window.run_until_parked();
        window.update(|window, cx| {
            assert_eq!(
                state.read(cx).value("opacity"),
                Some(&PropertyValue::Number(0.4))
            );
            state.update(cx, |state, cx| {
                assert!(state.undo(cx));
            });
            window.draw(cx).clear();
            let PropertyEditor::Number(editor) = &state.read(cx).editors["opacity"] else {
                panic!()
            };
            assert_eq!(editor.read(cx).value(), 1.0);
            assert_eq!(
                state.read(cx).value("name"),
                Some(&PropertyValue::Text("RenamedX🙂".into()))
            );
        });
    }
    #[::core::prelude::v1::test]
    fn rendered_checkbox_choices_and_undo_update_actual_property_values() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| {
            crate::init(cx);
            crate::theme::install_theme(cx, Theme::dark());
        });
        let state = cx.new(|cx| PropertyInspectorState::new(groups(), cx).unwrap());
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
                    .unwrap_or_else(|| panic!("missing {label} action"))
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
        click(window, "Visible");
        window.update(|_, cx| {
            state.update(cx, |state, _| {
                assert_eq!(state.value("visible"), Some(&PropertyValue::Boolean(false)))
            })
        });
        click(window, "Undo");
        window.update(|_, cx| {
            state.update(cx, |state, _| {
                assert_eq!(state.value("visible"), Some(&PropertyValue::Boolean(true)))
            })
        });
        click(window, "Mode choices");
        click(window, "Dark");
        window.update(|_, cx| {
            state.update(cx, |state, _| {
                assert_eq!(
                    state.value("mode"),
                    Some(&PropertyValue::Choice("dark".into()))
                )
            })
        });
        click(window, "Undo");
        window.update(|_, cx| {
            state.update(cx, |state, _| {
                assert_eq!(
                    state.value("mode"),
                    Some(&PropertyValue::Choice("light".into()))
                )
            })
        });
    }
}

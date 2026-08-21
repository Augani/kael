//! Select component - Dropdown select with keyboard navigation.

use crate::components::icon::Icon;
use crate::components::icon_source::IconSource;
use crate::components::input::InputSize;
use crate::components::{
    field::{Field, FieldStatusType},
    field_status::FieldStatusVariant,
    spinner::{Spinner, SpinnerSize},
};
use crate::theme::Theme;
use kael::{prelude::*, *};

actions!(select, [SelectUp, SelectDown, SelectConfirm, SelectCancel]);

const DROPDOWN_MARGIN: Pixels = px(4.0);

#[derive(Clone, Debug)]
pub enum SelectEvent {
    Change,
}

#[derive(Clone)]
pub struct SelectOption<T: Clone> {
    pub value: T,
    pub label: SharedString,
    pub group: Option<SharedString>,
    pub icon: Option<IconSource>,
    pub font_family: Option<SharedString>,
    pub disabled: bool,
}

impl<T: Clone> SelectOption<T> {
    pub fn new(value: T, label: impl Into<SharedString>) -> Self {
        Self {
            value,
            label: label.into(),
            group: None,
            icon: None,
            font_family: None,
            disabled: false,
        }
    }

    pub fn with_group(mut self, group: impl Into<SharedString>) -> Self {
        self.group = Some(group.into());
        self
    }

    pub fn with_icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Renders this option, and the selected value, in the supplied font.
    /// This is useful for font-family pickers where the visual preview is part
    /// of the choice rather than decorative content.
    pub fn with_font_family(mut self, family: impl Into<SharedString>) -> Self {
        self.font_family = Some(family.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

#[derive(Clone, Debug)]
pub struct SelectorOptionData {
    pub value: SharedString,
    pub label: Option<SharedString>,
    pub disabled: bool,
    pub icon: Option<IconSource>,
}

impl SelectorOptionData {
    pub fn new(value: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            label: None,
            disabled: false,
            icon: None,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn into_select_option(self) -> SelectOption<SharedString> {
        let label = self.label.unwrap_or_else(|| self.value.clone());
        let mut option = SelectOption::new(self.value, label);
        option.icon = self.icon;
        option.disabled = self.disabled;
        option
    }
}

#[derive(Clone, Debug, Default)]
pub struct SelectorDivider;

#[derive(Clone, Debug)]
pub struct SelectorSection {
    pub title: Option<SharedString>,
    pub options: Vec<SelectorOptionData>,
}

impl SelectorSection {
    pub fn new(options: impl IntoIterator<Item = SelectorOptionData>) -> Self {
        Self {
            title: None,
            options: options.into_iter().collect(),
        }
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn into_select_options(self) -> Vec<SelectOption<SharedString>> {
        let title = self.title;
        self.options
            .into_iter()
            .map(|option| {
                let mut option = option.into_select_option();
                option.group = title.clone();
                option
            })
            .collect()
    }
}

pub struct Select<T: Clone + 'static> {
    focus_handle: FocusHandle,
    scroll_handle: ScrollHandle,
    options: Vec<SelectOption<T>>,
    selected_index: Option<usize>,
    highlighted_index: Option<usize>,
    label: Option<SharedString>,
    label_hidden: bool,
    description: Option<SharedString>,
    placeholder: Option<SharedString>,
    open: bool,
    disabled: bool,
    optional: bool,
    required: bool,
    size: InputSize,
    searchable: bool,
    clearable: bool,
    loading: bool,
    status: Option<(FieldStatusType, SharedString)>,
    search_query: String,
    on_change: Option<Box<dyn Fn(&T, &mut Window, &mut App) + Send + Sync + 'static>>,
    bounds: Bounds<Pixels>,
    trigger_label_bounds: Bounds<Pixels>,
    dropdown_bounds: Bounds<Pixels>,
    leading_icon: Option<IconSource>,
    style: StyleRefinement,
}

impl<T: Clone + 'static> Select<T> {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            scroll_handle: ScrollHandle::new(),
            options: Vec::new(),
            selected_index: None,
            highlighted_index: None,
            label: None,
            label_hidden: false,
            description: None,
            placeholder: None,
            open: false,
            disabled: false,
            optional: false,
            required: false,
            size: InputSize::default(),
            searchable: false,
            clearable: false,
            loading: false,
            status: None,
            search_query: String::new(),
            on_change: None,
            bounds: Bounds::default(),
            trigger_label_bounds: Bounds::default(),
            dropdown_bounds: Bounds::default(),
            leading_icon: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn options(mut self, options: Vec<SelectOption<T>>) -> Self {
        self.options = options;
        self
    }

    pub fn selected_index(mut self, index: Option<usize>) -> Self {
        self.selected_index = index;
        self.highlighted_index = index;
        self
    }

    pub fn label<S: Into<SharedString>>(mut self, label: S) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn is_label_hidden(mut self, hidden: bool) -> Self {
        self.label_hidden = hidden;
        self
    }

    #[allow(non_snake_case)]
    pub fn isLabelHidden(self, hidden: bool) -> Self {
        self.is_label_hidden(hidden)
    }

    pub fn description<S: Into<SharedString>>(mut self, description: S) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn placeholder<S: Into<SharedString>>(mut self, placeholder: S) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    #[allow(non_snake_case)]
    pub fn isDisabled(self, disabled: bool) -> Self {
        self.disabled(disabled)
    }

    pub fn optional(mut self, optional: bool) -> Self {
        self.optional = optional;
        self
    }

    #[allow(non_snake_case)]
    pub fn isOptional(self, optional: bool) -> Self {
        self.optional(optional)
    }

    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    #[allow(non_snake_case)]
    pub fn isRequired(self, required: bool) -> Self {
        self.required(required)
    }

    pub fn size(mut self, size: InputSize) -> Self {
        self.size = size;
        self
    }

    pub fn searchable(mut self, searchable: bool) -> Self {
        self.searchable = searchable;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasSearch(self, searchable: bool) -> Self {
        self.searchable(searchable)
    }

    pub fn clearable(mut self, clearable: bool) -> Self {
        self.clearable = clearable;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasClear(self, clearable: bool) -> Self {
        self.clearable(clearable)
    }

    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    pub fn is_loading(self, loading: bool) -> Self {
        self.loading(loading)
    }

    #[allow(non_snake_case)]
    pub fn isLoading(self, loading: bool) -> Self {
        self.loading(loading)
    }

    pub fn status(mut self, status: FieldStatusType, message: impl Into<SharedString>) -> Self {
        self.status = Some((status, message.into()));
        self
    }

    pub fn on_change<F: Fn(&T, &mut Window, &mut App) + Send + Sync + 'static>(
        mut self,
        f: F,
    ) -> Self {
        self.on_change = Some(Box::new(f));
        self
    }

    pub fn leading_icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.leading_icon = Some(icon.into());
        self
    }

    pub fn start_icon(self, icon: impl Into<IconSource>) -> Self {
        self.leading_icon(icon)
    }

    #[allow(non_snake_case)]
    pub fn startIcon(self, icon: impl Into<IconSource>) -> Self {
        self.start_icon(icon)
    }

    pub fn selected_value(&self) -> Option<&T> {
        self.selected_index
            .and_then(|i| self.options.get(i))
            .map(|opt| &opt.value)
    }

    pub fn selected_label(&self) -> Option<&SharedString> {
        self.selected_index
            .and_then(|i| self.options.get(i))
            .map(|opt| &opt.label)
    }

    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open
    }

    fn filtered_options(&self) -> Vec<(usize, &SelectOption<T>)> {
        if self.search_query.is_empty() {
            self.options.iter().enumerate().collect()
        } else {
            let query_lower = self.search_query.to_lowercase();
            self.options
                .iter()
                .enumerate()
                .filter(|(_, opt)| opt.label.to_lowercase().contains(&query_lower))
                .collect()
        }
    }

    fn toggle_dropdown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.disabled && !self.loading {
            self.open = !self.open;
            if self.open {
                self.scroll_handle.set_offset(point(px(0.0), px(0.0)));
                window.focus(&self.focus_handle);
                self.highlighted_index = self
                    .selected_index
                    .filter(|index| {
                        self.options
                            .get(*index)
                            .is_some_and(|option| !option.disabled)
                    })
                    .or_else(|| {
                        self.filtered_options()
                            .into_iter()
                            .find_map(|(index, option)| (!option.disabled).then_some(index))
                    });
                self.scroll_highlighted_into_view();
            }
            cx.notify();
        }
    }

    /// Keep the highlighted option inside the visible rows of the dropdown
    /// viewport. Row geometry mirrors the render budget: 28px rows with 4px
    /// top padding, at most 12 visible rows; a negative y offset means the
    /// menu is scrolled down.
    fn scroll_highlighted_into_view(&mut self) {
        const ROW_HEIGHT: f32 = 28.0;
        const TOP_PADDING: f32 = 4.0;

        let Some(highlighted) = self.highlighted_index else {
            return;
        };
        let filtered = self.filtered_options();
        let Some(position) = filtered.iter().position(|(index, _)| *index == highlighted) else {
            return;
        };

        let visible_rows = filtered.len().min(12);
        let viewport_height = (visible_rows as f32)
            .mul_add(ROW_HEIGHT, 8.0)
            .clamp(48.0, 300.0);
        let row_top = TOP_PADDING + position as f32 * ROW_HEIGHT;
        let row_bottom = row_top + ROW_HEIGHT;
        let scrolled = -f32::from(self.scroll_handle.offset().y);

        let new_scrolled = if row_top < scrolled {
            (row_top - TOP_PADDING).max(0.0)
        } else if row_bottom > scrolled + viewport_height {
            row_bottom - viewport_height
        } else {
            return;
        };
        self.scroll_handle
            .set_offset(point(px(0.0), px(-new_scrolled.max(0.0))));
    }

    fn close_dropdown(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        self.dropdown_bounds = Bounds::default();
        self.highlighted_index = self.selected_index;
        self.search_query.clear();
        cx.notify();
    }

    fn clear_selection(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.selected_index = None;
        self.highlighted_index = None;

        cx.emit(SelectEvent::Change);
        cx.notify();
    }

    fn select_option(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .options
            .get(index)
            .is_some_and(|option| !option.disabled)
        {
            self.selected_index = Some(index);
            self.highlighted_index = Some(index);
            self.open = false;

            cx.emit(SelectEvent::Change);
            cx.notify();

            if let Some(ref cb) = self.on_change
                && let Some(option) = self.options.get(index)
            {
                (cb)(&option.value, window, cx);
            }
        }
    }

    fn select_up(&mut self, _: &SelectUp, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            self.toggle_dropdown(window, cx);
            return;
        }

        let filtered: Vec<_> = self
            .filtered_options()
            .into_iter()
            .filter(|(_, option)| !option.disabled)
            .collect();
        if filtered.is_empty() {
            return;
        }

        let current_pos = self
            .highlighted_index
            .and_then(|idx| filtered.iter().position(|(orig_idx, _)| *orig_idx == idx));

        let new_pos = match current_pos {
            Some(0) => filtered.len() - 1,
            Some(pos) => pos - 1,
            None => filtered.len() - 1,
        };

        self.highlighted_index = Some(filtered[new_pos].0);
        self.scroll_highlighted_into_view();
        cx.notify();
    }

    fn select_down(&mut self, _: &SelectDown, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            self.toggle_dropdown(window, cx);
            return;
        }

        let filtered: Vec<_> = self
            .filtered_options()
            .into_iter()
            .filter(|(_, option)| !option.disabled)
            .collect();
        if filtered.is_empty() {
            return;
        }

        let current_pos = self
            .highlighted_index
            .and_then(|idx| filtered.iter().position(|(orig_idx, _)| *orig_idx == idx));

        let new_pos = match current_pos {
            Some(pos) if pos < filtered.len() - 1 => pos + 1,
            Some(_) => 0,
            None => 0,
        };

        self.highlighted_index = Some(filtered[new_pos].0);
        self.scroll_highlighted_into_view();
        cx.notify();
    }

    fn select_confirm(&mut self, _: &SelectConfirm, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            if let Some(idx) = self.highlighted_index {
                self.select_option(idx, window, cx);
            }
        } else {
            self.toggle_dropdown(window, cx);
        }
    }

    fn select_cancel(&mut self, _: &SelectCancel, _: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            self.close_dropdown(cx);
        }
    }
}

impl<T: Clone + 'static> Styled for Select<T> {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl<T: Clone + 'static> Render for Select<T> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style.clone();

        let display_text = self
            .selected_label()
            .cloned()
            .or_else(|| self.placeholder.clone())
            .unwrap_or_else(|| "Select...".into());
        let trigger_display_text =
            SharedString::from(display_text.replace(['\r', '\n'], " ").trim().to_owned());

        let open = self.open;
        let highlighted_idx = self.highlighted_index;
        let bounds = self.bounds;
        let entity_id = cx.entity().entity_id().as_u64();
        let is_focused = self.focus_handle.is_focused(window);

        let maybe_selected_icon: Option<IconSource> = self
            .selected_index
            .and_then(|i| self.options.get(i))
            .and_then(|opt| opt.icon.clone());
        let leading_icon = self.leading_icon.clone().or(maybe_selected_icon);
        let display_font_family = self
            .selected_index
            .and_then(|index| self.options.get(index))
            .and_then(|option| option.font_family.clone())
            .unwrap_or_else(|| theme.tokens.font_family.clone());
        let hover_ring = crate::astryx::input_hover_ring(theme.tokens.input);
        let focus_ring = crate::astryx::focus_ring(theme.tokens.primary);
        let status = self.status.clone();
        let display_color = if self.selected_index.is_some() {
            theme.tokens.foreground
        } else {
            theme.tokens.muted_foreground
        };
        let status_color = status.as_ref().map(|(status, _)| match status {
            FieldStatusType::Warning => theme.tokens.warning,
            FieldStatusType::Error => theme.tokens.destructive,
            FieldStatusType::Success => theme.tokens.success,
        });
        let (height, padding_x, text_size, icon_size) = match self.size {
            InputSize::Sm => (px(28.0), px(8.0), px(14.0), px(16.0)),
            InputSize::Md => (px(32.0), px(12.0), px(14.0), px(16.0)),
            InputSize::Lg => (px(36.0), px(12.0), px(14.0), px(16.0)),
        };

        let mut accessibility_state = if open {
            AccessibilityState::EXPANDED
        } else {
            AccessibilityState::COLLAPSED
        };
        if self.disabled {
            accessibility_state |= AccessibilityState::DISABLED;
        }
        if self.required {
            accessibility_state |= AccessibilityState::REQUIRED;
        }
        if self.loading {
            accessibility_state |= AccessibilityState::BUSY;
        }
        if is_focused {
            accessibility_state |= AccessibilityState::FOCUSED;
        }
        if matches!(status.as_ref(), Some((FieldStatusType::Error, _))) {
            accessibility_state |= AccessibilityState::INVALID;
        }
        let accessibility_label = self
            .label
            .clone()
            .unwrap_or_else(|| SharedString::from("Select"));
        let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::ComboBox)
            .label(accessibility_label.to_string())
            .placeholder(
                self.placeholder
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "Select...".to_owned()),
            )
            .states(accessibility_state);
        if self.selected_index.is_some() {
            accessibility = accessibility.value(AccessibilityValue::Text(display_text.to_string()));
        }
        if !self.disabled && !self.loading {
            accessibility = accessibility.actions(vec![
                AccessibilityAction::Focus,
                AccessibilityAction::Click,
                if open {
                    AccessibilityAction::Collapse
                } else {
                    AccessibilityAction::Expand
                },
            ]);
        }

        let trigger_focus_handle = self
            .focus_handle
            .clone()
            .tab_index(if self.disabled { -1 } else { 0 })
            .tab_stop(!self.disabled);
        let trigger = div()
            .id(("select-trigger", entity_id))
            .relative()
            .track_focus(&trigger_focus_handle)
            .accessibility(accessibility)
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .h(height)
            .px(padding_x)
            .bg(theme.tokens.card)
            .border_1()
            .border_color(if let Some(color) = status_color {
                color
            } else if open {
                theme.tokens.primary
            } else {
                theme.tokens.input
            })
            .rounded(theme.tokens.radius_md)
            .text_color(if self.selected_index.is_some() {
                theme.tokens.foreground
            } else {
                theme.tokens.muted_foreground
            })
            .text_size(text_size)
            .font_family(display_font_family.clone())
            .shadow(smallvec::smallvec![crate::astryx::focus_ring(
                kael::transparent_black()
            )])
            .when(open && !self.disabled, |div| {
                div.shadow(smallvec::smallvec![focus_ring])
            })
            .cursor(if self.disabled {
                CursorStyle::Arrow
            } else {
                CursorStyle::PointingHand
            })
            .when(!self.disabled, |div: Stateful<Div>| {
                div.hover(move |style| {
                    style
                        .border_color(status_color.unwrap_or(theme.tokens.input))
                        .shadow(smallvec::smallvec![
                            status_color.map_or(hover_ring, crate::astryx::input_hover_ring)
                        ])
                })
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.toggle_dropdown(window, cx);
                }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .flex_1()
                    .min_w(px(0.0))
                    .when_some(leading_icon.as_ref(), |this, src| {
                        this.child(
                            div()
                                .size(px(16.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .flex_shrink_0()
                                .child(
                                    Icon::new(src.clone())
                                        .size(icon_size)
                                        .color(theme.tokens.muted_foreground),
                                ),
                        )
                    })
                    .child({
                        let entity = cx.entity().clone();
                        let display_font_family = display_font_family.clone();
                        canvas_with_prepaint(
                            move |bounds, window, cx| {
                                entity.update(cx, |this, _| {
                                    this.trigger_label_bounds = bounds;
                                });
                                let mut runs = vec![TextRun {
                                    len: trigger_display_text.len(),
                                    font: font(display_font_family.clone()),
                                    color: display_color,
                                    background_color: None,
                                    underline: None,
                                    strikethrough: None,
                                }];
                                let mut wrapper = window
                                    .text_system()
                                    .line_wrapper(font(display_font_family.clone()), text_size);
                                let text = wrapper.truncate_line(
                                    trigger_display_text,
                                    bounds.size.width.max(px(0.0)),
                                    "…",
                                    &mut runs,
                                );
                                window
                                    .text_system()
                                    .shape_line(text, text_size, &runs, None)
                            },
                            move |bounds, line, window, cx| {
                                let _ = line.paint(bounds.origin, bounds.size.height, window, cx);
                            },
                        )
                        .flex_1()
                        .min_w(px(0.0))
                        .h_full()
                    }),
            )
            .child(div().flex().items_center().justify_center().child(
                if self.clearable && self.selected_index.is_some() && !self.disabled {
                    div()
                        .size(px(24.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(theme.tokens.radius_sm)
                        .cursor(CursorStyle::PointingHand)
                        .hover(|mut style| {
                            style.background = Some(theme.tokens.muted.into());
                            style
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _event: &MouseDownEvent, window, cx| {
                                this.clear_selection(window, cx);
                                cx.stop_propagation();
                            }),
                        )
                        .accessibility(
                            AccessibilityAttributes::new(AccessibilityRole::Button)
                                .label("Clear selection")
                                .actions(vec![AccessibilityAction::Click]),
                        )
                        .child(
                            Icon::new("x")
                                .size(icon_size)
                                .color(theme.tokens.muted_foreground),
                        )
                        .into_any_element()
                } else if self.loading {
                    div()
                        .size(px(24.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Spinner::new().size(SpinnerSize::Sm))
                        .into_any_element()
                } else if let Some((status, _)) = status.as_ref() {
                    let (icon, color) = match status {
                        FieldStatusType::Warning => ("triangle-alert", theme.tokens.warning),
                        FieldStatusType::Error => ("circle-alert", theme.tokens.destructive),
                        FieldStatusType::Success => ("circle-check", theme.tokens.success),
                    };
                    div()
                        .size(px(24.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Icon::new(icon).size(icon_size).color(color))
                        .into_any_element()
                } else {
                    div()
                        .size(px(24.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            Icon::new(if open { "chevron-up" } else { "chevron-down" })
                                .size(icon_size)
                                .color(theme.tokens.muted_foreground),
                        )
                        .into_any_element()
                },
            ))
            .child({
                let entity = cx.entity().clone();
                canvas_with_prepaint(
                    move |bounds, _, cx| {
                        entity.update(cx, |this, _| {
                            this.bounds = bounds;
                        })
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full()
            });

        let searchable = self.searchable;
        let search_query: SharedString = self.search_query.clone().into();
        let visible_option_rows =
            u16::try_from(self.filtered_options().len().min(12)).unwrap_or(12);
        let option_viewport_height = f32::from(visible_option_rows)
            .mul_add(28.0, 8.0)
            .clamp(48.0, 300.0);
        let dropdown_width = bounds
            .size
            .width
            .max(px(if searchable { 300.0 } else { 220.0 }));
        if open {
            self.dropdown_bounds = Bounds::new(
                point(bounds.left(), bounds.bottom() + DROPDOWN_MARGIN),
                size(
                    dropdown_width,
                    px(option_viewport_height + if searchable { 45.0 } else { 0.0 }),
                ),
            );
        }

        let control = div()
            .relative()
            .w_full()
            .key_context("Select")
            .when(!self.disabled && !self.loading, |this: Div| {
                this.on_action(cx.listener(Select::select_up))
                    .on_action(cx.listener(Select::select_down))
                    .on_action(cx.listener(Select::select_confirm))
            })
            .when(open && !self.disabled && !self.loading, |this: Div| {
                this.on_action(cx.listener(Select::select_cancel))
            })
            .when(open && searchable, |this: Div| {
                this.on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                    if event.keystroke.key == "backspace" {
                        this.search_query.pop();
                        this.scroll_handle.set_offset(point(px(0.0), px(0.0)));
                        cx.notify();
                    }
                    else if event.keystroke.key.len() == 1 && !event.keystroke.modifiers.control && !event.keystroke.modifiers.platform {
                        this.search_query.push_str(&event.keystroke.key);
                        this.scroll_handle.set_offset(point(px(0.0), px(0.0)));
                        let filtered = this.filtered_options();
                        this.highlighted_index = filtered
                            .into_iter()
                            .find_map(|(index, option)| (!option.disabled).then_some(index));
                        cx.notify();
                    }
                }))
            })
            .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                if this.open && !this.dropdown_bounds.contains(&event.position) {
                    this.close_dropdown(cx);
                }
            }))
            .child(trigger)
            .when(open, |this| {
                this.child(
                    deferred(
                        anchored()
                            .snap_to_window_with_margin(Edges::all(DROPDOWN_MARGIN))
                            .child(
                                div()
                                    .relative()
                                    .occlude()
                                    .w(dropdown_width)
                                    .child({
                                        let entity = cx.entity().clone();
                                        canvas_with_prepaint(
                                            move |bounds, _, cx| {
                                                entity.update(cx, |this, _| {
                                                    this.dropdown_bounds = bounds;
                                                })
                                            },
                                            |_, _, _, _| {},
                                        )
                                        .absolute()
                                        .size_full()
                                    })
                                    .child(
                                        div()
                                            .relative()
                                            .w_full()
                                            .occlude()
                                            .mt(DROPDOWN_MARGIN)
                                            .bg(theme.tokens.popover)
                                            .border_1()
                                            .border_color(theme.tokens.border)
                                            .rounded(theme.tokens.radius_lg)
                                            .shadow(theme.tokens.shadow_lg.to_vec())
                                            .overflow_hidden()
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .when(searchable, |this| {
                                                        this.child(
                                                            div()
                                                                .px(px(12.0))
                                                                .pt(px(8.0))
                                                                .pb(px(4.0))
                                                                .border_b_1()
                                                                .border_color(theme.tokens.border)
                                                                .child(
                                                                    div()
                                                                        .w_full()
                                                                        .h(px(32.0))
                                                                        .px(px(8.0))
                                                                        .flex()
                                                                        .items_center()
                                                                        .bg(theme.tokens.card)
                                                                        .border_1()
                                                                        .border_color(theme.tokens.input)
                                                                        .rounded(theme.tokens.radius_sm)
                                                                        .text_size(px(13.0))
                                                                        .font_family(theme.tokens.font_family.clone())
                                                                        .text_color(if search_query.is_empty() {
                                                                            theme.tokens.muted_foreground
                                                                        } else {
                                                                            theme.tokens.foreground
                                                                        })
                                                                        .child(if search_query.is_empty() {
                                                                            SharedString::from("Type to search...")
                                                                        } else {
                                                                            search_query.clone()
                                                                        })
                                                                )
                                                        )
                                                    })
                                                    .child(
                                                        div()
                                                            .id("select-options-viewport")
                                                            .h(px(option_viewport_height))
                                                            .child(
                                                                div()
                                                                    .id(("select-options", entity_id))
                                                                    .size_full()
                                                                    .overflow_y_scroll()
                                                                    .track_scroll(&self.scroll_handle)
                                                                    .child({
                                                                let filtered = self.filtered_options();
                                                                let loading = self.loading;
                                                                let (item_padding_x, item_padding_y) = match self.size {
                                                                    InputSize::Sm => (px(8.0), px(4.0)),
                                                                    InputSize::Md => (px(8.0), px(6.0)),
                                                                    InputSize::Lg => (px(8.0), px(8.0)),
                                                                };

                                                                div()
                                                                    .py(px(4.0))
                                                                    .when(loading, |this| {
                                                                    this.child(
                                                                        div()
                                                                            .px(px(12.0))
                                                                            .py(px(16.0))
                                                                            .flex()
                                                                            .items_center()
                                                                            .justify_center()
                                                                            .gap(px(8.0))
                                                                            .text_size(px(13.0))
                                                                            .font_family(theme.tokens.font_family.clone())
                                                                            .text_color(theme.tokens.muted_foreground)
                                                                            .child(
                                                                                div()
                                                                                    .text_size(px(18.0))
                                                                                    .text_color(theme.tokens.primary)
                                                                                    .child("⟳")
                                                                            )
                                                                            .child("Loading options...")
                                                                    )
                                                                })
                                                                .when(!loading && filtered.is_empty(), |this| {
                                                                    this.child(
                                                                        div()
                                                                            .px(px(12.0))
                                                                            .py(px(16.0))
                                                                            .flex()
                                                                            .items_center()
                                                                            .justify_center()
                                                                            .text_size(px(13.0))
                                                                            .font_family(theme.tokens.font_family.clone())
                                                                            .text_color(theme.tokens.muted_foreground)
                                                                            .child("No results found")
                                                                    )
                                                                })
                                                                .when(!loading && !filtered.is_empty(), |this| {
                                                                    let mut current_group: Option<SharedString> = None;
                                                                    this.children(
                                                                        filtered.iter().flat_map(|(index, option)| {
                                                                            let mut elements = Vec::new();

                                                                            if option.group != current_group {
                                                                                current_group = option.group.clone();
                                                                                if let Some(group_name) = &option.group {
                                                                                    elements.push(
                                                                                        div()
                                                                                            .px(px(12.0))
                                                                                            .pt(px(12.0))
                                                                                            .pb(px(4.0))
                                                                                            .text_size(px(11.0))
                                                                                            .font_family(theme.tokens.font_family.clone())
                                                                                            .font_weight(FontWeight::SEMIBOLD)
                                                                                            .text_color(theme.tokens.muted_foreground)
                                                                                            .child(group_name.clone())
                                                                                            .into_any_element()
                                                                                    );
                                                                                }
                                                                            }

                                                                            let is_selected = self.selected_index == Some(*index);
                                                                            let is_highlighted = highlighted_idx == Some(*index);
                                                                            let is_disabled = option.disabled;
                                                                            let index = *index;
                                                                            let option_icon = option.icon.clone();
                                                                            let option_font_family = option
                                                                                .font_family
                                                                                .clone()
                                                                                .unwrap_or_else(|| theme.tokens.font_family.clone());
                                                                            let option_label = option.label.clone();
                                                                            let mut option_state = AccessibilityState::NONE;
                                                                            if is_selected {
                                                                                option_state |= AccessibilityState::SELECTED;
                                                                            }
                                                                            if is_highlighted {
                                                                                option_state |= AccessibilityState::FOCUSED;
                                                                            }
                                                                            if is_disabled {
                                                                                option_state |= AccessibilityState::DISABLED;
                                                                            }

                                                                            elements.push(
                                                                                div()
                                                                                    .id(ElementId::NamedInteger(
                                                                                        format!("select-option-{entity_id}").into(),
                                                                                        index as u64,
                                                                                    ))
                                                                                    .accessibility({
                                                                                        let mut attributes = AccessibilityAttributes::new(AccessibilityRole::ListItem)
                                                                                            .label(option_label.to_string())
                                                                                            .states(option_state);
                                                                                        if !is_disabled {
                                                                                            attributes = attributes.actions(vec![AccessibilityAction::Click]);
                                                                                        }
                                                                                        attributes
                                                                                    })
                                                                                    .px(item_padding_x)
                                                                                    .py(item_padding_y)
                                                                                    .flex()
                                                                                    .items_center()
                                                                                    .justify_between()
                                                                                    .gap(px(8.0))
                                                                                    .rounded(theme.tokens.radius_md)
                                                                                    .text_color(theme.tokens.popover_foreground)
                                                                                    .bg(if is_highlighted {
                                                                                        crate::astryx::overlay_hover(theme.tokens.background.l < 0.5)
                                                                                    } else {
                                                                                        kael::transparent_black()
                                                                                    })
                                                                                    .when(!is_disabled, |this| {
                                                                                        this.hover(|style| {
                                                                                            style.bg(crate::astryx::overlay_hover(
                                                                                                theme.tokens.background.l < 0.5,
                                                                                            ))
                                                                                        })
                                                                                    })
                                                                                    .cursor(if is_disabled {
                                                                                        CursorStyle::OperationNotAllowed
                                                                                    } else {
                                                                                        CursorStyle::PointingHand
                                                                                    })
                                                                                    .when(is_disabled, |this| this.opacity(0.5))
                                                                                    .text_size(px(14.0))
                                                                                    .when(is_selected, |this| {
                                                                                        this.font_weight(FontWeight::MEDIUM)
                                                                                    })
                                                                                    .font_family(option_font_family)
                                                                                    .when(!is_disabled, |this| {
                                                                                        this.on_mouse_down(MouseButton::Left, cx.listener(move |this, _, window, cx| {
                                                                                            this.select_option(index, window, cx);
                                                                                        }))
                                                                                    })
                                                                                    .child(
                                                                                        div()
                                                                                            .flex()
                                                                                            .items_center()
                                                                                            .gap(px(8.0))
                                                                                            .flex_1()
                                                                                            .min_w(px(0.0))
                                                                                            .when_some(option_icon, |row, src| {
                                                                                                row.child(
                                                                                                    div()
                                                                                                        .size(px(16.0))
                                                                                                        .flex()
                                                                                                        .items_center()
                                                                                                        .justify_center()
                                                                                                        .flex_shrink_0()
                                                                                                        .child(
                                                                                                            Icon::new(src)
                                                                                                                .size(px(16.0))
                                                                                                                .color(theme.tokens.muted_foreground)
                                                                                                        )
                                                                                                )
                                                                                            })
                                                                                            .child(
                                                                                                div()
                                                                                                    .flex_1()
                                                                                                    .min_w(px(0.0))
                                                                                                    .overflow_hidden()
                                                                                                    .text_ellipsis()
                                                                                                    .whitespace_nowrap()
                                                                                                    .child(option_label)
                                                                                            )
                                                                                    )
                                                                                    .child(
                                                                                        div()
                                                                                            .size(px(16.0))
                                                                                            .flex()
                                                                                            .items_center()
                                                                                            .justify_center()
                                                                                            .flex_shrink_0()
                                                                                            .when(is_selected, |slot| {
                                                                                                slot.child(
                                                                                                    Icon::new("check")
                                                                                                        .size(px(16.0))
                                                                                                        .color(theme.tokens.popover_foreground)
                                                                                                )
                                                                                            })
                                                                                    )
                                                                                    .into_any_element()
                                                                            );

                                                                            elements
                                                                        })
                                                                    )
                                                                })
                                                                    })
                                                        )
                                                    )
                                            ),
                                    ),
                            ),
                    )
                    .with_priority(1),
                )
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            });

        match self.label.clone() {
            Some(label) => {
                let mut field = Field::new(label, control)
                    .hidden_label(self.label_hidden)
                    .optional(self.optional)
                    .required(self.required)
                    .disabled(self.disabled)
                    .status_variant(FieldStatusVariant::Detached);

                if let Some(description) = self.description.clone() {
                    field = field.description(description);
                }

                if let Some((status, message)) = self.status.clone() {
                    field = field.status(status, message);
                }

                field.into_any_element()
            }
            None => control.into_any_element(),
        }
    }
}

/// Initialize select key bindings
pub fn init_select(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SelectUp, Some("Select")),
        KeyBinding::new("down", SelectDown, Some("Select")),
        KeyBinding::new("enter", SelectConfirm, Some("Select")),
        KeyBinding::new("space", SelectConfirm, Some("Select")),
        KeyBinding::new("escape", SelectCancel, Some("Select")),
    ]);
}

impl<T: Clone + 'static> Focusable for Select<T> {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl<T: Clone + 'static> EventEmitter<SelectEvent> for Select<T> {}

#[cfg(test)]
mod tests {
    use super::{Select, SelectOption, init_select};
    use kael::{
        AppContext as _, Context, Entity, Focusable as _, IntoElement, Modifiers, MouseButton,
        ParentElement as _, Render, ScrollDelta, ScrollWheelEvent, Styled as _, TestAppContext,
        Window, div, point, px, size,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct SelectHost {
        first: Entity<Select<&'static str>>,
        disabled: Entity<Select<&'static str>>,
        last: Entity<Select<&'static str>>,
    }

    struct DisabledSelectHost {
        select: Entity<Select<&'static str>>,
    }

    struct ScrollableSelectHost {
        select: Entity<Select<usize>>,
    }

    impl Render for SelectHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .child(self.first.clone())
                .child(self.disabled.clone())
                .child(self.last.clone())
        }
    }

    impl Render for DisabledSelectHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.select.clone()
        }
    }

    impl Render for ScrollableSelectHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(220.0)).child(self.select.clone())
        }
    }

    fn select(
        disabled: bool,
        changes: Arc<AtomicUsize>,
        cx: &mut kael::App,
    ) -> Entity<Select<&'static str>> {
        cx.new(|cx| {
            Select::new(cx)
                .options(vec![
                    SelectOption::new("one", "One"),
                    SelectOption::new("two", "Two"),
                ])
                .selected_index(Some(0))
                .disabled(disabled)
                .on_change(move |_, _, _| {
                    changes.fetch_add(1, Ordering::Relaxed);
                })
        })
    }

    #[kael::test]
    fn closed_select_opens_and_commits_once_from_keyboard(cx: &mut TestAppContext) {
        cx.update(|cx| {
            init_select(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let changes = Arc::new(AtomicUsize::new(0));
        let first = cx.update(|cx| select(false, changes.clone(), cx));
        let disabled = cx.update(|cx| select(true, Arc::new(AtomicUsize::new(0)), cx));
        let last = cx.update(|cx| select(false, Arc::new(AtomicUsize::new(0)), cx));
        let (_host, window) = cx.add_window_view({
            let first = first.clone();
            let disabled = disabled.clone();
            let last = last.clone();
            move |_, _| SelectHost {
                first,
                disabled,
                last,
            }
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
            window.focus(&first.read(cx).focus_handle(cx));
        });
        window.simulate_keystrokes("enter");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(first.read(cx).open, "Enter must open a closed Select");
        });
        window.simulate_keystrokes("down enter");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(!first.read(cx).open);
            assert_eq!(first.read(cx).selected_value(), Some(&"two"));
        });
        assert_eq!(
            changes.load(Ordering::Relaxed),
            1,
            "keyboard selection must commit once"
        );

        window.simulate_keystrokes("space");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(first.read(cx).open, "Space must open a closed Select");
        });
        window.simulate_keystrokes("escape down");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(
                first.read(cx).open,
                "Escape must close the Select before ArrowDown reopens it"
            );
        });
        assert_eq!(
            changes.load(Ordering::Relaxed),
            1,
            "open and cancel must not emit a change"
        );
    }

    #[kael::test]
    fn disabled_select_is_skipped_by_tab_navigation(cx: &mut TestAppContext) {
        cx.update(|cx| {
            init_select(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let disabled = cx.update(|cx| select(true, Arc::new(AtomicUsize::new(0)), cx));
        let (_host, window) = cx.add_window_view({
            let disabled = disabled.clone();
            move |_, _| DisabledSelectHost { select: disabled }
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
            window.focus_next();
            window.draw(cx).clear();
            assert!(!disabled.read(cx).focus_handle(cx).is_focused(window));
            assert!(window.focused(cx).is_none());
        });
    }

    #[kael::test]
    fn long_select_menu_scrolls_with_the_pointer_wheel(cx: &mut TestAppContext) {
        cx.update(|cx| {
            init_select(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let select = cx.new(|cx| {
            Select::new(cx).options(
                (0..80)
                    .map(|index| SelectOption::new(index, format!("Font {index:02}")))
                    .collect(),
            )
        });
        let (_host, window) = cx.add_window_view({
            let select = select.clone();
            move |_, _| ScrollableSelectHost { select }
        });
        window.simulate_resize(size(px(500.0), px(500.0)));
        window.update(|window, cx| {
            window.draw(cx).clear();
            window.focus(&select.read(cx).focus_handle(cx));
        });
        window.simulate_keystrokes("enter");
        window.update(|window, cx| {
            window.draw(cx).clear();
            window.draw(cx).clear();
        });

        let trigger_bounds = window.update(|_, cx| select.read(cx).bounds);
        let menu_point = point(
            trigger_bounds.origin.x + trigger_bounds.size.width / 2.0,
            trigger_bounds.origin.y + trigger_bounds.size.height + px(120.0),
        );
        window.simulate_event(ScrollWheelEvent {
            position: menu_point,
            delta: ScrollDelta::Pixels(point(px(0.0), px(-10_000.0))),
            modifiers: Modifiers::default(),
            ..Default::default()
        });
        window.update(|window, cx| window.draw(cx).clear());

        window.update(|_, cx| {
            assert!(
                select.read(cx).scroll_handle.offset().y < px(0.0),
                "a wheel gesture over a long Select menu must scroll its retained viewport"
            );
        });
    }

    #[kael::test]
    fn long_selected_value_stays_inside_trigger_and_menu_uses_a_wider_viewport(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            init_select(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let label = "An Exceptionally Long Installed Font Family Name";
        let select = cx.new(|cx| {
            Select::new(cx)
                .label("Font family")
                .is_label_hidden(true)
                .searchable(true)
                .options(vec![SelectOption::new(0, label)])
                .selected_index(Some(0))
                .w(px(72.0))
        });
        let (_host, window) = cx.add_window_view({
            let select = select.clone();
            move |_, _| ScrollableSelectHost { select }
        });
        window.simulate_resize(size(px(500.0), px(500.0)));
        window.update(|window, cx| {
            window.draw(cx).clear();
            let trigger = select.read(cx).bounds;
            let text = select.read(cx).trigger_label_bounds;
            assert!(text.top() >= trigger.top());
            assert!(text.bottom() <= trigger.bottom());
            assert!(text.size.height <= trigger.size.height);
            window.focus(&select.read(cx).focus_handle(cx));
        });
        window.simulate_keystrokes("enter");
        window.update(|window, cx| {
            window.draw(cx).clear();
            window.draw(cx).clear();
            assert!(select.read(cx).dropdown_bounds.size.width >= px(300.0));
        });
    }

    #[kael::test]
    fn arrow_key_navigation_scrolls_the_highlighted_option_into_view(cx: &mut TestAppContext) {
        cx.update(|cx| {
            init_select(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let select = cx.new(|cx| {
            Select::new(cx).options(
                (0..80)
                    .map(|index| SelectOption::new(index, format!("Font {index:02}")))
                    .collect(),
            )
        });
        let (_host, window) = cx.add_window_view({
            let select = select.clone();
            move |_, _| ScrollableSelectHost { select }
        });
        window.simulate_resize(size(px(500.0), px(500.0)));
        window.update(|window, cx| {
            window.draw(cx).clear();
            window.focus(&select.read(cx).focus_handle(cx));
        });
        window.simulate_keystrokes("enter");
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        window.update(|_, cx| {
            assert_eq!(
                select.read(cx).scroll_handle.offset().y,
                px(0.0),
                "opening on the first option must not scroll"
            );
        });

        // Arrow past the 12th visible row: the viewport must follow the
        // highlight instead of leaving it clipped below the fold.
        for _ in 0..13 {
            window.simulate_keystrokes("down");
        }
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        window.update(|_, cx| {
            let offset = f32::from(select.read(cx).scroll_handle.offset().y);
            assert!(
                offset < 0.0,
                "arrowing past the visible rows must scroll the highlight into view, got {offset}"
            );
            let scrolled = -offset;
            let highlighted_position = select
                .read(cx)
                .highlighted_index
                .expect("a highlight must exist after arrowing")
                as f32;
            let row_top = 4.0 + highlighted_position * 28.0;
            let row_bottom = row_top + 28.0;
            let visible_rows = 12;
            let viewport_height = (visible_rows as f32).mul_add(28.0, 8.0).clamp(48.0, 300.0);
            assert!(
                row_top >= scrolled - 0.5 && row_bottom <= scrolled + viewport_height + 0.5,
                "highlighted row [{row_top},{row_bottom}] must be visible in [{scrolled},{}]",
                scrolled + viewport_height
            );
        });

        // Arrow back up to the first row: the viewport must return to the top.
        for _ in 0..13 {
            window.simulate_keystrokes("up");
        }
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        window.update(|_, cx| {
            assert_eq!(
                select.read(cx).scroll_handle.offset().y,
                px(0.0),
                "arrowing back to the top must scroll the viewport back to the top"
            );
        });
    }

    #[kael::test]
    fn select_scrollbar_thumb_drag_keeps_menu_open_and_scrolls(cx: &mut TestAppContext) {
        cx.update(|cx| {
            init_select(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let select = cx.new(|cx| {
            Select::new(cx).searchable(true).options(
                (0..100)
                    .map(|index| SelectOption::new(index, format!("Font {index:03}")))
                    .collect(),
            )
        });
        let (_host, window) = cx.add_window_view({
            let select = select.clone();
            move |_, _| ScrollableSelectHost { select }
        });
        window.simulate_resize(size(px(500.0), px(500.0)));
        window.update(|window, cx| {
            window.draw(cx).clear();
            window.focus(&select.read(cx).focus_handle(cx));
        });
        window.simulate_keystrokes("enter");
        window.update(|window, cx| {
            window.draw(cx).clear();
            window.draw(cx).clear();
        });
        let menu = window.update(|_, cx| select.read(cx).dropdown_bounds);
        let thumb_start = point(menu.right() - px(4.0), menu.top() + px(58.0));
        let thumb_end = point(menu.right() - px(4.0), menu.top() + px(230.0));
        window.simulate_mouse_move(thumb_start, None, Modifiers::default());
        window.simulate_mouse_down(thumb_start, MouseButton::Left, Modifiers::default());
        window.simulate_mouse_move(thumb_end, Some(MouseButton::Left), Modifiers::default());
        window.simulate_mouse_up(thumb_end, MouseButton::Left, Modifiers::default());
        window.update(|window, cx| window.draw(cx).clear());
        window.update(|_, cx| {
            let select = select.read(cx);
            assert!(
                select.open,
                "dragging the menu scrollbar must not dismiss it"
            );
            assert!(
                select.scroll_handle.offset().y < px(0.0),
                "dragging the visible scrollbar thumb must move the menu viewport"
            );
        });
    }
}

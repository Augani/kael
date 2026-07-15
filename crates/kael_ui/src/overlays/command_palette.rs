//! Command palette component with fuzzy search.

use crate::{
    components::{
        icon::Icon,
        icon_source::IconSource,
        input::Input,
        input_state::InputState,
        scrollable::scrollable_vertical,
        text::{body, caption, label_small},
    },
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, InteractiveElement, *};
use std::{panic::Location, rc::Rc};

actions!(
    command_palette,
    [NavigateUp, NavigateDown, SelectCommand, CloseCommand]
);

pub fn init_command_palette(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", NavigateUp, Some("CommandPalette")),
        KeyBinding::new("down", NavigateDown, Some("CommandPalette")),
        KeyBinding::new("enter", SelectCommand, Some("CommandPalette")),
        KeyBinding::new("escape", CloseCommand, Some("CommandPalette")),
    ]);
}

#[derive(Clone)]
pub struct Command {
    pub id: SharedString,
    pub name: SharedString,
    pub description: Option<SharedString>,
    pub icon: Option<IconSource>,
    pub category: Option<SharedString>,
    pub shortcut: Option<SharedString>,
    pub on_select: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    search_text: String,
}

pub type CommandPaletteItem = Command;

#[derive(IntoElement)]
pub struct CommandPaletteInput {
    input: Entity<InputState>,
    placeholder: SharedString,
    end_content: Option<AnyElement>,
    busy: bool,
    style: StyleRefinement,
}

impl CommandPaletteInput {
    pub fn new(input: Entity<InputState>) -> Self {
        Self {
            input,
            placeholder: "Search...".into(),
            end_content: None,
            busy: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn end_content(mut self, content: impl IntoElement) -> Self {
        self.end_content = Some(content.into_any_element());
        self
    }

    pub fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }
}

impl Styled for CommandPaletteInput {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for CommandPaletteInput {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;

        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(16.0))
            .py(px(12.0))
            .border_b_1()
            .border_color(theme.tokens.border)
            .child(
                Icon::new("search")
                    .size(px(18.0))
                    .color(theme.tokens.muted_foreground),
            )
            .child(
                div()
                    .flex_1()
                    .child(Input::new(&self.input).placeholder(self.placeholder)),
            )
            .when(self.busy, |this| {
                this.child(
                    crate::components::spinner::Spinner::new()
                        .size(crate::components::spinner::SpinnerSize::Sm)
                        .shade(crate::components::spinner::SpinnerShade::Subtle),
                )
            })
            .children(self.end_content)
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[derive(IntoElement)]
pub struct CommandPaletteList {
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

impl CommandPaletteList {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }
}

impl Default for CommandPaletteList {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for CommandPaletteList {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for CommandPaletteList {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let user_style = self.style;

        div()
            .flex_1()
            .overflow_hidden()
            .child(scrollable_vertical(
                div().flex().flex_col().p(px(8.0)).children(self.children),
            ))
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[derive(IntoElement)]
pub struct CommandPaletteGroup {
    label: Option<SharedString>,
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

impl CommandPaletteGroup {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: Some(label.into()),
            children: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn unlabeled() -> Self {
        Self {
            label: None,
            children: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }
}

impl Styled for CommandPaletteGroup {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for CommandPaletteGroup {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;

        div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .when_some(self.label, |this, label| {
                this.child(
                    label_small(label)
                        .color(theme.tokens.muted_foreground)
                        .px(px(12.0))
                        .py(px(6.0)),
                )
            })
            .children(self.children)
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[derive(IntoElement)]
pub struct CommandPaletteFooter {
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

impl CommandPaletteFooter {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }
}

impl Default for CommandPaletteFooter {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for CommandPaletteFooter {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for CommandPaletteFooter {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;

        div()
            .flex()
            .items_center()
            .justify_between()
            .px(px(16.0))
            .py(px(8.0))
            .border_t_1()
            .border_color(theme.tokens.border)
            .bg(theme.tokens.muted.opacity(0.3))
            .children(self.children)
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[derive(IntoElement)]
pub struct CommandPaletteEmpty {
    message: SharedString,
    style: StyleRefinement,
}

impl CommandPaletteEmpty {
    pub fn new(message: impl Into<SharedString>) -> Self {
        Self {
            message: message.into(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for CommandPaletteEmpty {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for CommandPaletteEmpty {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;

        div()
            .flex()
            .items_center()
            .justify_center()
            .h(px(160.0))
            .px(px(16.0))
            .text_size(px(13.0))
            .line_height(relative(1.4))
            .text_color(theme.tokens.muted_foreground)
            .child(self.message)
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

impl Command {
    pub fn new(id: impl Into<SharedString>, name: impl Into<SharedString>) -> Self {
        let id = id.into();
        let name = name.into();
        let search_text = name.to_string().to_lowercase();

        Self {
            id,
            name,
            description: None,
            icon: None,
            category: None,
            shortcut: None,
            on_select: None,
            search_text,
        }
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        let desc = description.into();
        self.search_text = format!("{} {}", self.name, desc).to_lowercase();
        self.description = Some(desc);
        self
    }

    pub fn icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn category(mut self, category: impl Into<SharedString>) -> Self {
        self.category = Some(category.into());
        self
    }

    pub fn shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub fn on_select<F>(mut self, handler: F) -> Self
    where
        F: Fn(&mut Window, &mut App) + 'static,
    {
        self.on_select = Some(Rc::new(handler));
        self
    }

    pub fn matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }

        let query = query.to_lowercase();
        self.search_text.contains(&query)
    }

    pub fn match_score(&self, query: &str) -> i32 {
        if query.is_empty() {
            return 0;
        }

        let query = query.to_lowercase();
        let name_lower = self.name.to_string().to_lowercase();

        if name_lower == query {
            return 1000;
        }

        if name_lower.starts_with(&query) {
            return 500;
        }

        if name_lower.contains(&query) {
            return 100;
        }

        if self.search_text.contains(&query) {
            return 50;
        }

        0
    }

    /// Returns true when this command has a description.
    pub fn has_description(&self) -> bool {
        self.description.is_some()
    }

    /// Returns true when this command has an icon.
    pub fn has_icon(&self) -> bool {
        self.icon.is_some()
    }

    /// Returns true when this command belongs to a category.
    pub fn has_category(&self) -> bool {
        self.category.is_some()
    }

    /// Returns true when this command displays a shortcut.
    pub fn has_shortcut(&self) -> bool {
        self.shortcut.is_some()
    }

    /// Returns true when this command has a selection handler.
    pub fn has_select_handler(&self) -> bool {
        self.on_select.is_some()
    }

    /// Returns the command id length without exposing the id.
    pub fn id_len_bytes(&self) -> usize {
        self.id.len()
    }

    /// Returns the command name length without exposing the name.
    pub fn name_len_bytes(&self) -> usize {
        self.name.len()
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "command(id_len_bytes={}, name_len_bytes={}, has_description={}, has_icon={}, has_category={}, has_shortcut={}, has_select_handler={})",
            self.id_len_bytes(),
            self.name_len_bytes(),
            self.has_description(),
            self.has_icon(),
            self.has_category(),
            self.has_shortcut(),
            self.has_select_handler()
        )
    }
}

impl IntoElement for Command {
    type Element = AnyElement;

    fn into_element(self) -> Self::Element {
        let theme = crate::theme::use_theme();
        let overlay_hover = crate::astryx::overlay_hover(theme.tokens.background.l < 0.5);
        render_command_item_with_theme(self, false, overlay_hover, &theme).into_any_element()
    }
}

pub struct CommandPaletteState {
    commands: Vec<Command>,
    search_query: String,
    filtered_commands: Vec<Command>,
    selected_index: usize,
    recent_commands: Vec<SharedString>,
}

impl CommandPaletteState {
    pub fn new(commands: Vec<Command>) -> Self {
        let filtered_commands = commands.clone();

        Self {
            commands,
            search_query: String::new(),
            filtered_commands,
            selected_index: 0,
            recent_commands: Vec::new(),
        }
    }

    pub fn update_search(&mut self, query: String) {
        self.search_query = query.clone();

        if query.is_empty() {
            self.filtered_commands = self.commands.clone();
        } else {
            let mut matches: Vec<(Command, i32)> = self
                .commands
                .iter()
                .filter(|cmd| cmd.matches(&query))
                .map(|cmd| (cmd.clone(), cmd.match_score(&query)))
                .collect();

            matches.sort_by_key(|m| std::cmp::Reverse(m.1));
            self.filtered_commands = matches.into_iter().map(|(cmd, _)| cmd).collect();
        }

        self.selected_index = 0;
    }

    pub fn select_previous(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn select_next(&mut self) {
        if self.selected_index < self.filtered_commands.len().saturating_sub(1) {
            self.selected_index += 1;
        }
    }

    pub fn execute_selected(&mut self, window: &mut Window, cx: &mut App) -> bool {
        self.execute(self.selected_index, window, cx)
    }

    pub fn execute(&mut self, index: usize, window: &mut Window, cx: &mut App) -> bool {
        if let Some(command) = self.filtered_commands.get(index) {
            if let Some(handler) = &command.on_select {
                handler(window, cx);
                self.recent_commands.push(command.id.clone());
                if self.recent_commands.len() > 10 {
                    self.recent_commands.remove(0);
                }
                return true;
            }
        }
        false
    }

    pub fn filtered_commands(&self) -> &[Command] {
        &self.filtered_commands
    }

    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    /// Returns the total number of commands in the palette.
    pub fn command_count(&self) -> usize {
        self.commands.len()
    }

    /// Returns the number of commands matching the active query.
    pub fn filtered_count(&self) -> usize {
        self.filtered_commands.len()
    }

    /// Returns true when the active query is non-empty.
    pub fn has_query(&self) -> bool {
        !self.search_query.is_empty()
    }

    /// Returns the active query length without exposing the query.
    pub fn query_len_bytes(&self) -> usize {
        self.search_query.len()
    }

    /// Returns true when at least one command has a category.
    pub fn has_categories(&self) -> bool {
        self.commands.iter().any(Command::has_category)
    }

    /// Counts commands with shortcut labels.
    pub fn shortcut_count(&self) -> usize {
        self.commands
            .iter()
            .filter(|command| command.has_shortcut())
            .count()
    }

    /// Counts commands with selection handlers.
    pub fn executable_count(&self) -> usize {
        self.commands
            .iter()
            .filter(|command| command.has_select_handler())
            .count()
    }

    /// Returns the number of recent command ids retained by the palette.
    pub fn recent_count(&self) -> usize {
        self.recent_commands.len()
    }

    /// Returns true when the selected index points at a filtered command.
    pub fn has_selection(&self) -> bool {
        self.selected_index < self.filtered_commands.len()
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "command_palette_state(commands={}, filtered={}, has_query={}, query_len_bytes={}, selected_index={}, has_selection={}, recent={}, has_categories={}, shortcuts={}, executable={})",
            self.command_count(),
            self.filtered_count(),
            self.has_query(),
            self.query_len_bytes(),
            self.selected_index(),
            self.has_selection(),
            self.recent_count(),
            self.has_categories(),
            self.shortcut_count(),
            self.executable_count()
        )
    }
}

pub struct CommandPalette {
    id: ElementId,
    state: Entity<CommandPaletteState>,
    search_input: Entity<InputState>,
    on_close: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    focus_handle: FocusHandle,
    previous_focus_handle: Option<FocusHandle>,
    focused: bool,
    style: StyleRefinement,
}

impl CommandPalette {
    #[track_caller]
    pub fn new(_window: &mut Window, cx: &mut Context<Self>, commands: Vec<Command>) -> Self {
        Self::from_commands(cx, commands)
    }

    /// Create a command palette when a window handle is not otherwise needed by the caller.
    #[track_caller]
    pub fn from_commands(cx: &mut Context<Self>, commands: Vec<Command>) -> Self {
        let caller = Location::caller();
        let state = cx.new(|_| CommandPaletteState::new(commands));
        let search_input =
            cx.new(|cx| InputState::new(cx).placeholder("Type a command or search..."));
        let focus_handle = cx.focus_handle();

        cx.subscribe(&search_input, |this, _input, event, cx| {
            use crate::components::input_state::InputEvent;
            if let InputEvent::Change = event {
                let query = this.search_input.read(cx).content().to_string();
                this.state.update(cx, |state, _cx| {
                    state.update_search(query);
                });
                cx.notify();
            }
        })
        .detach();

        Self {
            id: ElementId::Name(
                format!(
                    "command-palette:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            state,
            search_input,
            on_close: None,
            focus_handle,
            previous_focus_handle: None,
            focused: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn on_close<F>(mut self, handler: F) -> Self
    where
        F: Fn(&mut Window, &mut App) + 'static,
    {
        self.on_close = Some(Rc::new(handler));
        self
    }

    fn handle_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(previous_focus_handle) = self.previous_focus_handle.take() {
            window.focus(&previous_focus_handle);
        }
        self.focused = false;
        self.search_input.update(cx, |input, cx| input.clear(cx));
        if let Some(handler) = self.on_close.as_ref() {
            handler(window, cx);
        }
    }
}

impl Styled for CommandPalette {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Focusable for CommandPalette {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for CommandPalette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let state = self.state.read(cx);
        let filtered = state.filtered_commands();
        let selected_idx = state.selected_index();
        let user_style = self.style.clone();
        let overlay_hover = crate::astryx::overlay_hover(theme.tokens.background.l < 0.5);
        let palette_id = self.id.clone();
        let state_entity = self.state.clone();
        let palette_entity = cx.entity();

        if !self.focused {
            self.previous_focus_handle = window.focused(cx);
            let input_focus = self.search_input.read(cx).focus_handle(cx);
            window.focus(&input_focus);
            self.focused = true;
        }

        div()
            .id(ElementId::NamedChild(
                Box::new(palette_id.clone()),
                "overlay".into(),
            ))
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(kael::rgba(0x00000088))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _event, window, cx| {
                    this.handle_close(window, cx);
                }),
            )
            .on_scroll_wheel(|_, _, _| {})
            .key_context("CommandPalette")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &NavigateUp, _window, cx| {
                this.state.update(cx, |state, _cx| {
                    state.select_previous();
                });
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &NavigateDown, _window, cx| {
                this.state.update(cx, |state, _cx| {
                    state.select_next();
                });
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &SelectCommand, window, cx| {
                let executed = this
                    .state
                    .update(cx, |state, app_cx| state.execute_selected(window, app_cx));
                if executed {
                    this.handle_close(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &CloseCommand, window, cx| {
                this.handle_close(window, cx);
            }))
            .child(
                div()
                    .id(palette_id.clone())
                    .accessibility(
                        AccessibilityAttributes::new(AccessibilityRole::Dialog)
                            .label("Command palette"),
                    )
                    .w(relative(0.9))
                    .max_w(px(640.0))
                    .max_h(relative(0.8))
                    .flex()
                    .flex_col()
                    .bg(theme.tokens.popover)
                    .rounded(theme.tokens.radius_lg)
                    .shadow(theme.tokens.shadow_lg.to_vec())
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, |_event, _window, _cx| {})
                    .map(|this| {
                        let mut div = this;
                        div.style().refine(&user_style);
                        div
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .px(px(16.0))
                            .py(px(12.0))
                            .border_b_1()
                            .border_color(theme.tokens.border)
                            .child(
                                Icon::new("search")
                                    .size(px(16.0))
                                    .color(theme.tokens.muted_foreground),
                            )
                            .child(
                                Input::new(&self.search_input)
                                    .placeholder("Type a command or search..."),
                            ),
                    )
                    .child(
                        div().flex_1().overflow_hidden().child(scrollable_vertical(
                            div()
                                .flex()
                                .flex_col()
                                .p(px(8.0))
                                .children(filtered.is_empty().then(|| {
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .h(px(200.0))
                                        .child(
                                            caption("No commands found")
                                                .color(theme.tokens.muted_foreground),
                                        )
                                        .into_any_element()
                                }))
                                .children(filtered.iter().enumerate().map(|(idx, command)| {
                                    let is_selected = idx == selected_idx;
                                    let mut command = command.clone();
                                    let state_entity = state_entity.clone();
                                    let palette_entity = palette_entity.clone();
                                    command.on_select = Some(Rc::new(move |window, cx| {
                                        let executed = state_entity
                                            .update(cx, |state, cx| state.execute(idx, window, cx));
                                        if executed {
                                            palette_entity.update(cx, |palette, cx| {
                                                palette.handle_close(window, cx);
                                            });
                                        }
                                    }));
                                    render_command_item(command, is_selected, overlay_hover, cx)
                                })),
                        )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .px(px(16.0))
                            .py(px(8.0))
                            .border_t_1()
                            .border_color(theme.tokens.border)
                            .child(
                                div()
                                    .flex()
                                    .gap(px(16.0))
                                    .child(
                                        label_small("↑↓ Navigate")
                                            .color(theme.tokens.muted_foreground),
                                    )
                                    .child(
                                        label_small("↵ Select")
                                            .color(theme.tokens.muted_foreground),
                                    )
                                    .child(
                                        label_small("Esc Close")
                                            .color(theme.tokens.muted_foreground),
                                    ),
                            ),
                    ),
            )
    }
}

fn render_command_item(
    command: Command,
    selected: bool,
    overlay_hover: Hsla,
    cx: &App,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    render_command_item_with_theme(command, selected, overlay_hover, theme)
}

fn render_command_item_with_theme(
    command: Command,
    selected: bool,
    overlay_hover: Hsla,
    theme: &Theme,
) -> impl IntoElement {
    let enabled = command.on_select.is_some();
    let item_id = ElementId::Name(format!("command-palette-item:{}", command.id).into());
    let mut state = AccessibilityState::NONE;
    if selected {
        state |= AccessibilityState::SELECTED;
    }
    if command.on_select.is_none() {
        state |= AccessibilityState::DISABLED;
    }
    let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::Button)
        .label(command.name.to_string())
        .states(state);
    if let Some(description) = command.description.as_ref() {
        accessibility = accessibility.description(description.to_string());
    }
    if command.on_select.is_some() {
        accessibility = accessibility.actions(vec![AccessibilityAction::Click]);
    }

    div()
        .id(item_id)
        .accessibility(accessibility)
        .flex()
        .items_center()
        .gap(px(8.0))
        .px(px(12.0))
        .py(px(8.0))
        .rounded(theme.tokens.radius_md)
        .when(enabled, |div| div.cursor(CursorStyle::PointingHand))
        .when(!enabled, |div| div.cursor(CursorStyle::Arrow).opacity(0.55))
        .when(selected, |div| div.bg(theme.tokens.accent))
        .when(!selected && enabled, |div| {
            div.hover(move |style| style.bg(overlay_hover))
        })
        .when_some(command.on_select, |div, handler| {
            div.on_click(move |_event, window, cx| {
                handler(window, cx);
            })
        })
        .when_some(command.icon, |div, icon| {
            div.child(
                Icon::new(icon)
                    .size(px(16.0))
                    .color(theme.tokens.muted_foreground),
            )
        })
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    body(command.name)
                        .accessibility_hidden(true)
                        .color(theme.tokens.foreground),
                )
                .when_some(command.description, |div, desc| {
                    div.child(
                        caption(desc)
                            .accessibility_hidden(true)
                            .color(theme.tokens.muted_foreground),
                    )
                }),
        )
        .children(command.shortcut.map(|shortcut| {
            div()
                .px(px(8.0))
                .py(px(4.0))
                .rounded(theme.tokens.radius_sm)
                .bg(theme.tokens.muted)
                .child(caption(shortcut).color(theme.tokens.muted_foreground))
                .into_any_element()
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn command_summary_is_content_safe() {
        let command = Command::new("private.open-secret-project", "Open Secret Project")
            .description("Opens the private workspace")
            .category("Private Workspaces")
            .shortcut("cmd-shift-p")
            .on_select(|_, _| {});

        assert!(command.has_description());
        assert!(command.has_category());
        assert!(command.has_shortcut());
        assert!(command.has_select_handler());
        assert_eq!(command.id_len_bytes(), "private.open-secret-project".len());
        assert_eq!(command.name_len_bytes(), "Open Secret Project".len());

        let summary = command.to_text();
        assert!(summary.contains("has_description=true"));
        assert!(summary.contains("has_category=true"));
        assert!(summary.contains("has_shortcut=true"));
        assert!(summary.contains("has_select_handler=true"));
        assert!(!summary.contains("private.open-secret-project"));
        assert!(!summary.contains("Open Secret Project"));
        assert!(!summary.contains("Private Workspaces"));
        assert!(!summary.contains("cmd-shift-p"));
    }

    #[::core::prelude::v1::test]
    fn command_palette_state_summary_is_content_safe() {
        let mut state = CommandPaletteState::new(vec![
            Command::new("private.open-secret-project", "Open Secret Project")
                .description("Opens the private workspace")
                .category("Private Workspaces")
                .shortcut("cmd-shift-p")
                .on_select(|_, _| {}),
            Command::new("public.help", "Show Help").category("Help"),
            Command::new("private.close-secret-project", "Close Secret Project"),
        ]);

        state.update_search("secret project".to_string());

        assert_eq!(state.command_count(), 3);
        assert_eq!(state.filtered_count(), 2);
        assert!(state.has_query());
        assert_eq!(state.query_len_bytes(), "secret project".len());
        assert!(state.has_selection());
        assert!(state.has_categories());
        assert_eq!(state.shortcut_count(), 1);
        assert_eq!(state.executable_count(), 1);
        assert_eq!(state.recent_count(), 0);

        let summary = state.to_text();
        assert!(summary.contains("commands=3"));
        assert!(summary.contains("filtered=2"));
        assert!(summary.contains("has_query=true"));
        assert!(summary.contains("shortcuts=1"));
        assert!(summary.contains("executable=1"));
        assert!(!summary.contains("secret project"));
        assert!(!summary.contains("Open Secret Project"));
        assert!(!summary.contains("private.open-secret-project"));
        assert!(!summary.contains("Private Workspaces"));
        assert!(!summary.contains("cmd-shift-p"));
    }
}

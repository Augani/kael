use kael::{prelude::FluentBuilder as _, *};
use kael_ui::astryx::ControlSize;
use kael_ui::components::alert::Alert;
use kael_ui::components::button_group::{ButtonGroup, ButtonGroupItem, ButtonGroupOrientation};
use kael_ui::components::code_block::CodeBlock;
use kael_ui::components::color_picker::{ColorPicker, ColorPickerState};
use kael_ui::components::date_picker::{DatePicker, DatePickerState};
use kael_ui::components::field::FieldStatusType;
use kael_ui::components::file_upload::{FileUpload, FileUploadState};
use kael_ui::components::input::Input;
use kael_ui::components::input_state::InputState;
use kael_ui::components::number_input::{NumberInput, NumberInputState};
use kael_ui::components::otp_input::{OTPInput, OTPState};
use kael_ui::components::pagination::Pagination;
use kael_ui::components::rating::{Rating, RatingState};
use kael_ui::components::scrollable::scrollable_vertical;
use kael_ui::components::segmented_nav::{SegmentedNav, SegmentedNavState};
use kael_ui::components::select::Select;
use kael_ui::components::slider::{Slider, SliderState};
use kael_ui::components::stepper::{StepItem, Stepper, StepperState};
use kael_ui::components::tag_input::{TagInput, TagInputState};
use kael_ui::components::text::{body, caption, code, h1, h2, h3, h4, h5, h6, label, muted, Text};
use kael_ui::components::time_picker::{TimePicker, TimePickerState};
use kael_ui::components::toggle_group::{ToggleGroup, ToggleGroupItem, ToggleGroupVariant};
use kael_ui::components::tooltip::{tooltip, Tooltip, TooltipFocusTrigger, TooltipHoverIndication};
use kael_ui::display::accordion::Accordion;
use kael_ui::display::table::{
    pixel, proportional, Table, TableBody, TableCell, TableColumn, TableColumnAlign, TableDensity,
    TableDividers, TableFooter, TableHeader, TableHeaderCell, TableRow, TableTextOverflow,
    TableVerticalAlign,
};
use kael_ui::navigation::tabs::{TabVariant, Tabs, TabsLayout, TabsSize};
use kael_ui::overlays::dialog::{Dialog, DialogPosition, DialogPurpose};
use kael_ui::overlays::hover_card::{HoverCard, HoverCardFocusTrigger, HoverCardHoverIndication};
use kael_ui::overlays::popover::{Popover, PopoverContent};
use kael_ui::overlays::sheet::{Sheet, SheetSide};
use kael_ui::overlays::toast::{
    Toast, ToastItem, ToastManager, ToastPosition, ToastType, ToastVariant, ToastViewport,
};
use kael_ui::prelude::*;
use kael_ui::prelude::{
    create_static_source, register_icons, AppShell, AppShellVariant, Avatar, AvatarGroup,
    AvatarGroupOverflow, AvatarItem, AvatarSize, AvatarStatusDot, AvatarStatusDotVariant, Badge,
    BadgeVariant, Banner, BannerContainer, BannerStatus, Button, ButtonSize, ButtonVariant,
    Calendar, Card, Chat, ChatMessage, ChatMessageRole, Checkbox, CheckboxList, CheckboxListItem,
    Citation, CitationVariant, ClickableCard, Code, CodeVariant, Collapsible, CollapsibleGroup,
    CommandPaletteEmpty, CommandPaletteFooter, CommandPaletteGroup, CommandPaletteInput,
    CommandPaletteItem, CommandPaletteList, ContextMenu, ContextMenuItem, DateValue, DayOfWeek,
    Divider, DividerVariant, DropdownMenuItemData, EmptyState, Grid, GridAlignment, GridSpan,
    Heading, HeadingLevel, HeadingType, Hue, Icon, IconButton, IconColor, IconRegistry, IconSize,
    InputGroup, InputGroupText, InputSize, InteractiveRole, InteractiveRoleContext, Item, Layer,
    LayerAlignment, LayerPlacement, LayerProvider, LayerToastConfig, Layout, LayoutContent,
    LayoutHeader, LayoutPanel, Link, List, ListStyle, ListVariant, MetadataList,
    MetadataListColumns, MetadataListItem, MobileNav, MobileNavToggle, NavItem, Outline,
    OutlineItem, OverflowList, Overlay, OverlayAlign, OverlayPosition, OverlayScrimMode,
    PaginationSize, PaginationVariant, PowerSearch, PowerSearchConfig, PowerSearchField,
    PowerSearchFilter, PowerSearchOperator, PowerSearchSize, ProgressBar, ProgressBarVariant,
    Radio, RadioGroup, ResizeHandle, SearchableItem, SegmentedControlItem, SegmentedControlLayout,
    SelectableCard, SelectorOption, Separator, SideNav, SideNavCollapseButton, SideNavHeading,
    SideNavItem, Skeleton, SkeletonRadius, SkeletonVariant, Spinner, SpinnerShade, SpinnerSize,
    SpinnerVariant, Stack, StackItem, StackItemSize, StatusDot, StatusDotVariant,
    SwitchLabelPosition, SwitchLabelSpacing, Tab, TabList, TabListLayout, TabListSize, TextColor,
    TextInputType, TextSize, TextType, TextWeight, Thumbnail, Timeline, TimelineItem, Toggle,
    Token, TokenColor, Tokenizer, TokenizerItem, TokenizerOverflowBehavior, Toolbar, ToolbarSize,
    TopNav, TopNavHeading, TopNavItem, TreeList, TreeListDensity, TreeNode, Typeahead,
    TypeaheadItem, KBD,
};
use kael_ui::theme::{install_theme, use_theme, Theme, ThemeTokens, ThemeVariant};
use std::path::PathBuf;

use kael::Axis;
use kael_ui::components::label::Label;
use kael_ui::components::progress::SpinnerType;
use kael_ui::components::resizable::{h_resizable, resizable_panel};
use kael_ui::components::split_pane::SplitDirection;
use kael_ui::navigation::menu::{Menu, MenuItem};
use kael_ui::navigation::status_bar::{StatusBar, StatusItem};

struct AstryxShowcase {
    terms: bool,
    notifications: bool,
    marketing: bool,
    plan: usize,
    card_pick: usize,
    page: usize,
    acc_open: std::collections::HashSet<usize>,
    segmented: Entity<SegmentedNavState>,
    slider: Entity<SliderState>,
    select: Entity<Select<String>>,
    number: Entity<NumberInputState>,
    rating: Entity<RatingState>,
    stepper: Entity<StepperState>,
    otp: Entity<OTPState>,
    tags: Entity<TagInputState>,
    date: Entity<DatePickerState>,
    file_state: Entity<FileUploadState>,
    color_state: Entity<ColorPickerState>,
    time_state: Entity<TimePickerState>,
    command_input: Entity<InputState>,
    field_search: Entity<InputState>,
    field_email: Entity<InputState>,
    field_invalid: Entity<InputState>,
    field_disabled: Entity<InputState>,
    field_textarea: Entity<InputState>,
    actions_copy: Entity<CopyButtonState>,
    actions_fab: Entity<FABState>,
    inputs_combobox: Entity<Combobox<String>>,
    inputs_search: Entity<SearchInput>,
    inputs_range: Entity<RangeSliderState>,
    selection_toggle_value: SharedString,
    selection_toggle_views: Vec<SharedString>,
    selection_checks: Vec<SharedString>,
    selection_switch_active: usize,
    selection_dropdown: Entity<DropdownState>,
    dd_expandable: Entity<ExpandableCardState>,
    dd_shortcuts: Entity<KeyboardShortcuts>,
    fb_countdown: Entity<CountdownState>,
    fb_counter: Entity<AnimatedCounterState>,
    fb_notifications: Entity<NotificationCenterState>,
    fb_skeleton: Entity<SkeletonLoaderState>,
    nav_menu_bar: Entity<MenuBar>,
    nav_status_bar: Entity<StatusBar>,
    overlays_alert_dialog: Entity<AlertDialog>,
    layout_collapsible_open: bool,
    layout_collapsible_a: bool,
    layout_collapsible_b: bool,
    layout_split: Entity<SplitPaneState>,
    layout_resizable: Entity<ResizableState>,
    layout_carousel: Entity<CarouselState>,
    category: ComponentCategory,
    show_dialog: bool,
    show_sheet: bool,
    dialog: Entity<Dialog>,
    sheet: Entity<Sheet>,
    toasts: Entity<ToastManager>,
    toast_n: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ComponentCategory {
    Actions,
    Inputs,
    Selection,
    DataDisplay,
    Charts,
    Feedback,
    Navigation,
    Overlays,
    Typography,
    Media,
    Layout,
}

impl ComponentCategory {
    const ALL: [ComponentCategory; 11] = [
        ComponentCategory::Actions,
        ComponentCategory::Inputs,
        ComponentCategory::Selection,
        ComponentCategory::DataDisplay,
        ComponentCategory::Charts,
        ComponentCategory::Feedback,
        ComponentCategory::Navigation,
        ComponentCategory::Overlays,
        ComponentCategory::Typography,
        ComponentCategory::Media,
        ComponentCategory::Layout,
    ];

    fn id(self) -> &'static str {
        match self {
            ComponentCategory::Actions => "actions",
            ComponentCategory::Inputs => "inputs",
            ComponentCategory::Selection => "selection",
            ComponentCategory::DataDisplay => "data-display",
            ComponentCategory::Charts => "charts",
            ComponentCategory::Feedback => "feedback",
            ComponentCategory::Navigation => "navigation",
            ComponentCategory::Overlays => "overlays",
            ComponentCategory::Typography => "typography",
            ComponentCategory::Media => "media",
            ComponentCategory::Layout => "layout",
        }
    }

    fn label(self) -> &'static str {
        match self {
            ComponentCategory::Actions => "Actions",
            ComponentCategory::Inputs => "Inputs & Forms",
            ComponentCategory::Selection => "Selection",
            ComponentCategory::DataDisplay => "Data Display",
            ComponentCategory::Charts => "Charts",
            ComponentCategory::Feedback => "Feedback",
            ComponentCategory::Navigation => "Navigation",
            ComponentCategory::Overlays => "Overlays",
            ComponentCategory::Typography => "Typography",
            ComponentCategory::Media => "Media",
            ComponentCategory::Layout => "Layout",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            ComponentCategory::Actions => "zap",
            ComponentCategory::Inputs => "text-cursor-input",
            ComponentCategory::Selection => "circle-check",
            ComponentCategory::DataDisplay => "table",
            ComponentCategory::Charts => "chart-column",
            ComponentCategory::Feedback => "bell",
            ComponentCategory::Navigation => "menu",
            ComponentCategory::Overlays => "layers",
            ComponentCategory::Typography => "type",
            ComponentCategory::Media => "image",
            ComponentCategory::Layout => "layout-grid",
        }
    }
}

impl AstryxShowcase {
    fn new(cx: &mut Context<Self>) -> Self {
        let mut icon_registry = IconRegistry::new();
        icon_registry.insert("brandSpark".to_string(), "sparkles".into());
        register_icons(icon_registry);

        let slider = cx.new(|cx| {
            let mut s = SliderState::new(cx);
            s.set_value(60.0, cx);
            s
        });
        let select = cx.new(|cx| {
            Select::new(cx)
                .placeholder("Select a country")
                .size(InputSize::Md)
                .options(vec![
                    SelectorOption::new("us".to_string(), "United States"),
                    SelectorOption::new("gh".to_string(), "Ghana"),
                    SelectorOption::new("jp".to_string(), "Japan"),
                    SelectorOption::new("se".to_string(), "Sweden"),
                ])
        });
        let stepper = cx.new(|cx| {
            StepperState::new(cx).with_steps(vec![
                StepItem::new("Account"),
                StepItem::new("Profile"),
                StepItem::new("Confirm"),
            ])
        });
        let view = cx.entity();
        let dialog = cx.new(|cx| {
            let view = view.clone();
            Dialog::new(cx)
                .title("Delete project?")
                .description("This permanently removes the project and all of its data.")
                .purpose(DialogPurpose::Form)
                .position(DialogPosition::new().top(px(80.0)))
                .on_close(move |_window, cx| {
                    view.update(cx, |this, cx| {
                        this.show_dialog = false;
                        cx.notify();
                    });
                })
        });
        let sheet = cx.new(|cx| {
            let view = view.clone();
            Sheet::new(cx)
                .side(SheetSide::Right)
                .title("Edit profile")
                .description("Update your account details and preferences.")
                .on_close(move |_window, cx| {
                    view.update(cx, |this, cx| {
                        this.show_sheet = false;
                        cx.notify();
                    });
                })
        });
        let toasts = cx.new(|cx| ToastManager::new(cx).position(ToastPosition::BottomRight));
        let inputs_combobox_state = cx.new(|_| ComboboxState::new());
        Self {
            terms: true,
            notifications: true,
            marketing: false,
            plan: 1,
            card_pick: 1,
            page: 3,
            acc_open: std::collections::HashSet::from([0]),
            segmented: cx.new(|_cx| SegmentedNavState::new("grid")),
            slider,
            select,
            number: cx.new(NumberInputState::new),
            rating: cx.new(RatingState::new),
            stepper,
            otp: cx.new(|cx| OTPState::new(cx, 6)),
            tags: cx.new(TagInputState::new),
            date: cx.new(|cx| DatePickerState::new(cx)),
            file_state: cx.new(|_| FileUploadState::new()),
            color_state: cx.new(|_| ColorPickerState::new(kael::hsla(0.62, 0.7, 0.5, 1.0))),
            time_state: cx.new(TimePickerState::new),
            command_input: cx.new(|cx| InputState::new(cx).placeholder("Search commands...")),
            field_search: cx.new(|cx| InputState::new(cx).placeholder("Search…")),
            field_email: cx.new(|cx| InputState::new(cx).placeholder("you@example.com")),
            field_invalid: cx.new(|cx| InputState::new(cx).placeholder("Required field")),
            field_disabled: cx.new(|cx| InputState::new(cx).placeholder("Disabled")),
            field_textarea: cx.new(|cx| InputState::new(cx).placeholder("Write a description...")),
            actions_copy: cx.new(|_| CopyButtonState::new("npm install kael_ui".into())),
            actions_fab: cx.new(|_| FABState::new()),
            inputs_combobox: cx.new(|cx| {
                Combobox::new(
                    vec![
                        "Rust".to_string(),
                        "Swift".to_string(),
                        "TypeScript".to_string(),
                        "Go".to_string(),
                        "Zig".to_string(),
                    ],
                    &inputs_combobox_state,
                    cx,
                )
                .placeholder("Pick a language")
                .render_item(|item| item.clone().into())
                .filter_fn(|item, search| item.to_lowercase().contains(search))
            }),
            inputs_search: cx.new(SearchInput::new),
            inputs_range: cx.new(RangeSliderState::new),
            selection_toggle_value: "bold".into(),
            selection_toggle_views: vec!["grid".into()],
            selection_checks: vec!["analytics".into(), "updates".into()],
            selection_switch_active: 0,
            selection_dropdown: cx.new(DropdownState::new),
            dd_expandable: cx.new(|_| ExpandableCardState::new()),
            dd_shortcuts: cx.new(|_| KeyboardShortcuts::new().category("Editing", vec![ShortcutItem::new("Copy", "cmd-c"), ShortcutItem::new("Paste", "cmd-v"), ShortcutItem::new("Undo", "cmd-z")]).category("Navigation", vec![ShortcutItem::new("Command palette", "cmd-shift-p"), ShortcutItem::new("Go to file", "cmd-p")])),
            fb_countdown: cx.new(|cx| { let mut s = CountdownState::new(cx); s.set_duration(std::time::Duration::from_secs(2 * 86400 + 5 * 3600 + 30 * 60 + 15), cx); s }),
            fb_counter: cx.new(|_| AnimatedCounterState::new(1280.0)),
            fb_notifications: cx.new(|cx| { let mut s = NotificationCenterState::new(cx); s.add(NotificationItem::new("fb-n3", "Deployment succeeded").message("v2.4.0 is live in production.").variant(NotificationVariant::Success), cx); s.add(NotificationItem::new("fb-n2", "Storage almost full").message("You have used 92% of your quota.").variant(NotificationVariant::Warning), cx); s.add(NotificationItem::new("fb-n1", "New comment on your PR").variant(NotificationVariant::Info), cx); s }),
            fb_skeleton: cx.new(|_| SkeletonLoaderState::new()),
            nav_menu_bar: cx.new(|_cx| MenuBar::new(vec![
    MenuBarItem::new("file", "File").with_items(vec![
        MenuItem::new("new", "New File").with_icon("file-plus").with_shortcut("\u{2318}N"),
        MenuItem::new("open", "Open").with_icon("folder-open").with_shortcut("\u{2318}O"),
        MenuItem::separator(),
        MenuItem::new("quit", "Quit").with_shortcut("\u{2318}Q"),
    ]),
    MenuBarItem::new("edit", "Edit").with_items(vec![
        MenuItem::new("undo", "Undo").with_shortcut("\u{2318}Z"),
        MenuItem::new("redo", "Redo").disabled(true),
    ]),
    MenuBarItem::new("view", "View"),
])),
            nav_status_bar: cx.new(|_cx| StatusBar::new()
    .left(vec![
        StatusItem::icon_text("git-branch", "main"),
        StatusItem::icon_text("circle-dot", "3 issues"),
    ])
    .center(vec![StatusItem::text("UTF-8")])
    .right(vec![
        StatusItem::badge("2", "2 warnings").badge_variant(BadgeVariant::Warning),
        StatusItem::icon_text("check-circle", "Ready"),
    ])),
            overlays_alert_dialog: cx.new(|cx| AlertDialog::new(cx).title("Delete project?").description("This permanently removes the project and all of its files. This action cannot be undone.").cancel_text("Cancel").action_text("Delete").destructive(true)),
            layout_collapsible_open: true,
            layout_collapsible_a: true,
            layout_collapsible_b: false,
            layout_split: cx.new(|cx| SplitPaneState::new(cx)),
            layout_resizable: ResizableState::new(cx),
            layout_carousel: cx.new(|cx| CarouselState::new(cx)),
            category: ComponentCategory::Actions,
            show_dialog: false,
            show_sheet: false,
            dialog,
            sheet,
            toasts,
            toast_n: 0,
        }
    }
}

fn section(title: &str, subtitle: &str, theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(16.0))
        .p(px(24.0))
        .bg(theme.tokens.card)
        .border_1()
        .border_color(theme.tokens.border)
        .rounded(theme.tokens.radius_lg)
        .shadow(theme.tokens.shadow_xs.to_vec())
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(h3(title.to_string()))
                .child(caption(subtitle.to_string())),
        )
}

fn row() -> Div {
    div().flex().flex_wrap().gap(px(12.0)).items_center()
}

fn col() -> Div {
    div().flex().flex_col().gap(px(10.0))
}

fn label_chip(text: &str, theme: &Theme) -> Div {
    div()
        .text_size(px(11.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.tokens.muted_foreground)
        .child(text.to_string())
}

fn theme_pill(
    label: &str,
    active: bool,
    tokens: ThemeTokens,
    theme: &Theme,
    on: impl Fn(&mut App) + 'static,
) -> impl IntoElement {
    let swatch = tokens.primary;
    div()
        .id(SharedString::from(format!("theme-{label}")))
        .flex()
        .items_center()
        .gap(px(8.0))
        .h(px(32.0))
        .px(px(12.0))
        .rounded(theme.tokens.radius_md)
        .border_1()
        .cursor_pointer()
        .when(active, |this| {
            this.border_color(theme.tokens.ring).bg(theme.tokens.accent)
        })
        .when(!active, |this| this.border_color(theme.tokens.border))
        .child(div().size(px(12.0)).rounded_full().bg(swatch))
        .child(
            div()
                .text_size(px(13.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.tokens.foreground)
                .child(label.to_string()),
        )
        .on_click(move |_, _, cx| on(cx))
}

impl Render for AstryxShowcase {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = use_theme();
        let variant = theme.variant;
        let view = cx.entity();

        let header = div()
            .flex()
            .flex_col()
            .gap(px(16.0))
            .p(px(24.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(h1("Astryx for Kael".to_string()))
                    .child(muted(
                        "Facebook's open design system, rebuilt for the desktop.".to_string(),
                    )),
            )
            .child(
                row()
                    .child(label_chip("Theme", &theme))
                    .child(theme_pill(
                        "Neutral",
                        variant == ThemeVariant::AstryxNeutral,
                        ThemeTokens::astryx_neutral(),
                        &theme,
                        |cx| install_theme(cx, Theme::astryx_neutral()),
                    ))
                    .child(theme_pill(
                        "Neutral Dark",
                        variant == ThemeVariant::AstryxNeutralDark,
                        ThemeTokens::astryx_neutral_dark(),
                        &theme,
                        |cx| install_theme(cx, Theme::astryx_neutral_dark()),
                    ))
                    .child(theme_pill(
                        "Blue",
                        variant == ThemeVariant::Astryx,
                        ThemeTokens::astryx(),
                        &theme,
                        |cx| install_theme(cx, Theme::astryx()),
                    ))
                    .child(theme_pill(
                        "Blue Dark",
                        variant == ThemeVariant::AstryxDark,
                        ThemeTokens::astryx_dark(),
                        &theme,
                        |cx| install_theme(cx, Theme::astryx_dark()),
                    )),
            );

        let buttons = section(
            "Buttons",
            "Six variants, three sizes, icons & states",
            &theme,
        )
        .child(
            row()
                .child(Button::new("b-default", "Primary"))
                .child(Button::new("b-secondary", "Secondary").variant(ButtonVariant::Secondary))
                .child(
                    Button::new("b-destructive", "Destructive").variant(ButtonVariant::Destructive),
                )
                .child(Button::new("b-outline", "Outline").variant(ButtonVariant::Outline))
                .child(Button::new("b-ghost", "Ghost").variant(ButtonVariant::Ghost))
                .child(Button::new("b-link", "Link").variant(ButtonVariant::Link)),
        )
        .child(
            row()
                .child(Button::new("b-sm", "Small").size(ButtonSize::Sm))
                .child(Button::new("b-md", "Medium").size(ButtonSize::Md))
                .child(Button::new("b-lg", "Large").size(ButtonSize::Lg))
                .child(
                    Button::new("b-icon", "")
                        .size(ButtonSize::Icon)
                        .icon("plus"),
                )
                .child(Button::new("b-loading", "Loading").loading(true))
                .child(Button::new("b-disabled", "Disabled").disabled(true)),
        );

        let badges = section(
            "Badges",
            "Solid status badges and the categorical hue palette",
            &theme,
        )
        .child(
            row()
                .child(Badge::new("Default"))
                .child(Badge::new("Secondary").variant(BadgeVariant::Secondary))
                .child(Badge::new("Success").variant(BadgeVariant::Success))
                .child(Badge::new("Warning").variant(BadgeVariant::Warning))
                .child(Badge::new("Destructive").variant(BadgeVariant::Destructive))
                .child(Badge::new("Outline").variant(BadgeVariant::Outline)),
        )
        .child(
            row().children(
                Hue::ALL
                    .iter()
                    .map(|h| Badge::new(h.label()).hue(*h).into_any_element()),
            ),
        );

        let inputs = section(
            "Text inputs",
            "Click and type — sizes, types and validation",
            &theme,
        )
        .child(
            row()
                .items_end()
                .child(
                    col().child(label_chip("Search (small)", &theme)).child(
                        div().w(px(220.0)).child(
                            Input::new(&self.field_search)
                                .size(InputSize::Sm)
                                .placeholder("Search…")
                                .start_icon(Icon::new("search")),
                        ),
                    ),
                )
                .child(
                    col().child(label_chip("Email", &theme)).child(
                        div().w(px(240.0)).child(
                            Input::new(&self.field_email)
                                .text_type(TextInputType::Email)
                                .placeholder("you@example.com"),
                        ),
                    ),
                )
                .child(
                    col().child(label_chip("Invalid", &theme)).child(
                        div().w(px(240.0)).child(
                            Input::new(&self.field_invalid)
                                .status(FieldStatusType::Error, "This field is required")
                                .placeholder("Required field"),
                        ),
                    ),
                )
                .child(
                    col().child(label_chip("Disabled", &theme)).child(
                        div().w(px(220.0)).child(
                            Input::new(&self.field_disabled)
                                .disabled(true)
                                .placeholder("Disabled"),
                        ),
                    ),
                ),
        );

        let _text_input_type: TextInputType = TextInputType::Email;
        let _dropdown_item_data: DropdownMenuItemData =
            DropdownMenuItemData::new("docs", "Open docs").icon("book-open");

        let selection = section("Selection", "Checkbox, radio and switch", &theme).child(
            row()
                .gap(px(40.0))
                .items_start()
                .child(
                    col()
                        .child(label_chip("Checkbox", &theme))
                        .child(
                            Checkbox::new("cb-terms")
                                .label("Accept terms")
                                .checked(self.terms)
                                .on_click({
                                    let view = view.clone();
                                    move |checked, _, cx| {
                                        let c = *checked;
                                        view.update(cx, |this, cx| {
                                            this.terms = c;
                                            cx.notify();
                                        });
                                    }
                                }),
                        )
                        .child(
                            Checkbox::new("cb-ind")
                                .label("Indeterminate")
                                .indeterminate(true),
                        )
                        .child(
                            Checkbox::new("cb-dis")
                                .label("Disabled")
                                .disabled(true)
                                .checked(true),
                        ),
                )
                .child(
                    col().child(label_chip("Radio", &theme)).child(
                        RadioGroup::new("plan")
                            .selected_index(Some(self.plan))
                            .on_change({
                                let view = view.clone();
                                move |ix, _, cx| {
                                    let ix = *ix;
                                    view.update(cx, |this, cx| {
                                        this.plan = ix;
                                        cx.notify();
                                    });
                                }
                            })
                            .child(Radio::new("free").label("Free"))
                            .child(Radio::new("pro").label("Pro"))
                            .child(Radio::new("team").label("Team")),
                    ),
                )
                .child(
                    col()
                        .child(label_chip("Switch", &theme))
                        .child(
                            Toggle::new("sw-notify")
                                .label("Notifications")
                                .checked(self.notifications)
                                .on_click({
                                    let view = view.clone();
                                    move |checked, _, cx| {
                                        let c = *checked;
                                        view.update(cx, |this, cx| {
                                            this.notifications = c;
                                            cx.notify();
                                        });
                                    }
                                }),
                        )
                        .child(
                            Toggle::new("sw-mkt")
                                .label("Marketing email")
                                .checked(self.marketing)
                                .label_position(SwitchLabelPosition::Start)
                                .label_spacing(SwitchLabelSpacing::Spread)
                                .on_click({
                                    let view = view.clone();
                                    move |checked, _, cx| {
                                        let c = *checked;
                                        view.update(cx, |this, cx| {
                                            this.marketing = c;
                                            cx.notify();
                                        });
                                    }
                                }),
                        )
                        .child(Toggle::new("sw-dis").label("Disabled").disabled(true)),
                ),
        );

        let feedback = section("Alerts", "Soft, sentiment-tinted banners", &theme)
            .child(
                Alert::info()
                    .title("Heads up")
                    .description("Astryx is now the default Kael theme."),
            )
            .child(
                Alert::success()
                    .title("Saved")
                    .description("Your changes have been published."),
            )
            .child(
                Alert::warning()
                    .title("Careful")
                    .description("This action affects every workspace member."),
            )
            .child(
                Alert::error()
                    .title("Something went wrong")
                    .description("We couldn't reach the server. Try again."),
            );

        let extras = section(
            "Banners, status & selectable cards",
            "New Astryx building blocks",
            &theme,
        )
        .child(Banner::announcement(
            "Astryx components now ship as part of Kael UI.",
        ))
        .child(Banner::new("All systems operational.").variant(BannerStatus::Success))
        .child(
            Banner::new("BannerContainer maps to the Astryx container API.")
                .container(BannerContainer::Card),
        )
        .child(
            Banner::warning("Configuration needs review")
                .description("Expanded banners render a card-backed content area.")
                .defaultIsExpanded(true)
                .child(
                    div()
                        .text_size(px(13.0))
                        .line_height(px(18.0))
                        .child("Review the pending workspace policy changes before publishing."),
                ),
        )
        .child(
            row()
                .gap(px(24.0))
                .child(
                    StatusDot::success()
                        .label("Online")
                        .tooltip("Online")
                        .isPulsing(true),
                )
                .child(StatusDot::new(StatusDotVariant::Warning).label("Degraded"))
                .child(StatusDot::error().label("Offline"))
                .child(StatusDot::accent().label("Accent"))
                .child(StatusDot::neutral().label("Neutral"))
                .child(StatusDot::new(StatusDotVariant::Hue(Hue::Purple)).label("Beta")),
        )
        .child(
            row()
                .gap(px(12.0))
                .items_start()
                .child(
                    div().w(px(210.0)).child(
                        SelectableCard::new("sc-free")
                            .selected(self.card_pick == 0)
                            .content(
                                col()
                                    .child(h3("Free".to_string()))
                                    .child(muted("For hobby projects".to_string())),
                            )
                            .on_click({
                                let view = view.clone();
                                move |_, _, cx| {
                                    view.update(cx, |this, cx| {
                                        this.card_pick = 0;
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
                )
                .child(
                    div().w(px(210.0)).child(
                        SelectableCard::new("sc-pro")
                            .selected(self.card_pick == 1)
                            .content(
                                col()
                                    .child(h3("Pro".to_string()))
                                    .child(muted("For growing teams".to_string())),
                            )
                            .on_click({
                                let view = view.clone();
                                move |_, _, cx| {
                                    view.update(cx, |this, cx| {
                                        this.card_pick = 1;
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
                ),
        );

        let misc = section(
            "Progress, spinners & avatars",
            "Loading and identity",
            &theme,
        )
        .child(
            row()
                .gap(px(32.0))
                .items_center()
                .child(
                    div().w(px(220.0)).child(
                        ProgressBar::new(0.0)
                            .value(62.0)
                            .max(100.0)
                            .label("Migration progress")
                            .isLabelHidden(true)
                            .hasValueLabel(true)
                            .variant(ProgressBarVariant::Success),
                    ),
                )
                .child(Spinner::new().size(SpinnerSize::Md))
                .child(
                    Spinner::new()
                        .size(SpinnerSize::Md)
                        .variant(SpinnerVariant::Primary),
                )
                .child(
                    Spinner::new()
                        .size(SpinnerSize::Md)
                        .shade(SpinnerShade::Subtle),
                )
                .child(
                    row()
                        .gap(px(8.0))
                        .child(
                            Avatar::new()
                                .name("Augustus Otu")
                                .size(AvatarSize::Md)
                                .status_dot(
                                    AvatarStatusDot::new()
                                        .variant(AvatarStatusDotVariant::Success)
                                        .label("Online"),
                                ),
                        )
                        .child(
                            Avatar::new()
                                .name("Kael UI")
                                .size(AvatarSize::Md)
                                .status_dot(
                                    AvatarStatusDot::new()
                                        .variant(AvatarStatusDotVariant::Neutral)
                                        .label("Away"),
                                ),
                        )
                        .child(
                            Avatar::new()
                                .name("Astryx")
                                .size(AvatarSize::Md)
                                .status_dot(
                                    AvatarStatusDot::new()
                                        .variant(AvatarStatusDotVariant::Error)
                                        .label("Busy"),
                                ),
                        ),
                )
                .child(
                    row()
                        .gap(px(8.0))
                        .child(KBD::new("mod+k"))
                        .child(KBD::new("mod+shift+p")),
                ),
        )
        .child(
            row()
                .gap(px(12.0))
                .items_center()
                .child(
                    Icon::new("success")
                        .size(IconSize::Md)
                        .icon_color(IconColor::Success),
                )
                .child(
                    Icon::new("warning")
                        .size(IconSize::Md)
                        .icon_color(IconColor::Warning),
                )
                .child(
                    Icon::new("error")
                        .size(IconSize::Md)
                        .icon_color(IconColor::Error),
                )
                .child(
                    Icon::new("brandSpark")
                        .size(IconSize::Lg)
                        .icon_color(IconColor::Accent),
                ),
        );

        let cards = section("Cards", "Containers with header, body and footer", &theme).child(
            row()
                .gap(px(16.0))
                .items_start()
                .child(
                    div().w(px(280.0)).child(
                        Card::new()
                            .header(h3("Project Apollo".to_string()))
                            .content(body(
                                "A cross-platform desktop app built with Kael and the Astryx design language."
                                    .to_string(),
                            ))
                            .footer(
                                row()
                                    .gap(px(8.0))
                                    .child(Button::new("c-open", "Open").size(ButtonSize::Sm))
                                    .child(
                                        Button::new("c-share", "Share")
                                            .size(ButtonSize::Sm)
                                            .variant(ButtonVariant::Outline),
                                    ),
                            ),
                    ),
                )
                .child(
                    div().w(px(280.0)).child(
                        Card::new()
                            .hoverable("card-hover")
                            .header(
                                row()
                                    .justify_between()
                                    .w_full()
                                    .child(h3("Status".to_string()))
                                    .child(Badge::new("Live").hue(Hue::Green)),
                            )
                            .content(
                                col()
                                    .child(body("Uptime 99.98%".to_string()))
                                    .child(muted("Last incident 42 days ago".to_string())),
                            ),
                    ),
                ),
        );

        let typography = section("Typography", "Figtree-scale headings and text", &theme)
            .child(
                col()
                    .gap(px(6.0))
                    .child(h1("Heading 1".to_string()))
                    .child(h2("Heading 2".to_string()))
                    .child(h3("Heading 3".to_string()))
                    .child(h4("Heading 4".to_string()))
                    .child(h5("Heading 5".to_string()))
                    .child(h6("Heading 6".to_string())),
            )
            .child(
                col()
                    .gap(px(4.0))
                    .child(body(
                        "Body text — the workhorse size for paragraphs and UI copy.".to_string(),
                    ))
                    .child(label(
                        "Label — used on form fields and metadata.".to_string(),
                    ))
                    .child(caption(
                        "Caption — secondary, lower-emphasis text.".to_string(),
                    ))
                    .child(muted("Muted — de-emphasized supporting text.".to_string()))
                    .child(code("let astryx = Theme::astryx();".to_string()))
                    .child(
                        Text::new("ASTRYX Text type/size/color API".to_string())
                            .text_type(TextType::Supporting)
                            .text_size(TextSize::Xsm)
                            .text_color(TextColor::Secondary)
                            .text_weight(TextWeight::Medium),
                    ),
            )
            .child(
                div()
                    .relative()
                    .h(px(96.0))
                    .w(px(320.0))
                    .overflow_hidden()
                    .rounded(theme.tokens.radius_lg)
                    .border_1()
                    .border_color(theme.tokens.border)
                    .child(
                        Layer::new()
                            .placement(LayerPlacement::Above)
                            .alignment(LayerAlignment::Center)
                            .surface(true)
                            .child(
                                div()
                                    .px(px(10.0))
                                    .py(px(6.0))
                                    .child(label("Layer placement=above".to_string())),
                            ),
                    ),
            )
            .child(
                LayerProvider::new()
                    .toast(LayerToastConfig {
                        position: ToastPosition::TopEnd,
                        max_visible: 3,
                        inset: px(8.0),
                    })
                    .child(label("LayerProvider wraps layer systems.".to_string())),
            );

        let details = section(
            "Avatars, skeletons, dividers & tooltips",
            "Identity, loading and structure",
            &theme,
        )
        .child(
            row()
                .gap(px(32.0))
                .items_center()
                .child(
                    AvatarGroup::new(vec![
                        AvatarItem::new().name("Augustus Otu"),
                        AvatarItem::new().name("Kael UI"),
                        AvatarItem::new().name("Astryx Design"),
                        AvatarItem::new().name("Meta Open Source"),
                        AvatarItem::new().name("Desktop Native"),
                    ])
                    .max_visible(3)
                    .size(AvatarSize::Md),
                )
                .child(AvatarGroupOverflow::new(2).size(AvatarSize::Md))
                .child(tooltip(
                    Button::new("tt-btn", "Hover for tooltip").variant(ButtonVariant::Outline),
                    "Astryx-styled tooltip",
                ))
                .child(
                    Tooltip::new("ASTRYX placement, alignment and hover indication")
                        .placement(LayerPlacement::Above)
                        .alignment(LayerAlignment::Center)
                        .focusTrigger(TooltipFocusTrigger::Auto)
                        .hasHoverIndication(TooltipHoverIndication::Always)
                        .isDefaultOpen(true)
                        .child(
                            Button::new("tt-astryx", "Default open tooltip")
                                .variant(ButtonVariant::Outline),
                        ),
                ),
        )
        .child(
            col()
                .gap(px(10.0))
                .child(
                    Skeleton::new()
                        .variant(SkeletonVariant::Text)
                        .radius(SkeletonRadius::R2)
                        .w(px(260.0))
                        .h(px(12.0)),
                )
                .child(
                    Skeleton::new()
                        .variant(SkeletonVariant::Text)
                        .radius(SkeletonRadius::Rounded)
                        .w(px(200.0))
                        .h(px(12.0)),
                )
                .child(
                    row()
                        .gap(px(12.0))
                        .items_center()
                        .child(
                            Skeleton::new()
                                .variant(SkeletonVariant::Circle)
                                .size(px(40.0)),
                        )
                        .child(
                            Skeleton::new()
                                .variant(SkeletonVariant::Rect)
                                .w(px(160.0))
                                .h(px(48.0)),
                        ),
                ),
        )
        .child(
            row()
                .gap(px(12.0))
                .items_start()
                .child(
                    Thumbnail::new()
                        .src("https://images.unsplash.com/photo-1498050108023-c5249f4df085?w=128&h=128&fit=crop")
                        .alt("Workspace preview")
                        .label("workspace.png")
                        .onRemove(|_, _| {}),
                )
                .child(Thumbnail::new().label("loading.png").isLoading(true))
                .child(Thumbnail::new().label("disabled.png").isDisabled(true))
                .child(Thumbnail::new().label("placeholder.png")),
        )
        .child(Separator::new().label("section divider"));

        let nav_disclosure = section(
            "Navigation & disclosure",
            "Pagination and accordion",
            &theme,
        )
        .child(
            Pagination::new()
                .current_page(self.page)
                .total_pages(10)
                .on_page_change({
                    let view = view.clone();
                    move |p, _, cx| {
                        view.update(cx, |this, cx| {
                            this.page = p;
                            cx.notify();
                        });
                    }
                }),
        )
        .child(
            row()
                .gap(px(12.0))
                .child(
                    Pagination::new()
                        .page(3)
                        .total_items(248)
                        .page_size(25)
                        .variant(PaginationVariant::Count)
                        .size(PaginationSize::Sm),
                )
                .child(
                    Pagination::new()
                        .page(3)
                        .total_pages(12)
                        .variant(PaginationVariant::Compact)
                        .size(PaginationSize::Sm),
                ),
        )
        .child(
            Pagination::new()
                .page(4)
                .total_pages(8)
                .variant(PaginationVariant::Dots)
                .page_size_options(vec![10, 25, 50])
                .page_size(25)
                .size(PaginationSize::Sm),
        )
        .child(
            Accordion::new("astryx-acc")
                .item(|item| {
                    item.title("What is Astryx?")
                        .icon("info")
                        .content(body(
                            "An open, fully-customizable design system from Meta.".to_string(),
                        ))
                        .open(self.acc_open.contains(&0))
                })
                .item(|item| {
                    item.title("Is it flexible?")
                        .icon("settings")
                        .content(body(
                            "Yes — every token and component style can be overridden.".to_string(),
                        ))
                        .open(self.acc_open.contains(&1))
                })
                .on_change(cx.listener(|this, indices: &[usize], _window, cx| {
                    this.acc_open = indices.iter().copied().collect();
                    cx.notify();
                })),
        );

        let controls = section(
            "Controls",
            "Segmented control, button group and slider",
            &theme,
        )
        .child(
            SegmentedNav::new("seg-view", self.segmented.clone())
                .layout(SegmentedControlLayout::Fill)
                .control_item(SegmentedControlItem::new("grid", "Grid"))
                .control_item(SegmentedControlItem::new("list", "List"))
                .control_item(SegmentedControlItem::new("table", "Table")),
        )
        .child(
            ButtonGroup::new("bg-edit")
                .size(ControlSize::Md)
                .child(
                    ButtonGroupItem::new("Copy")
                        .icon("copy")
                        .on_click(|_, _| {}),
                )
                .child(
                    ButtonGroupItem::new("Cut")
                        .icon("scissors")
                        .on_click(|_, _| {}),
                )
                .child(
                    ButtonGroupItem::new("Paste")
                        .icon("clipboard")
                        .on_click(|_, _| {}),
                ),
        )
        .child(
            ButtonGroup::new("bg-vertical")
                .orientation(ButtonGroupOrientation::Vertical)
                .size(ControlSize::Sm)
                .child(ButtonGroupItem::new("Preview").icon("eye"))
                .child(ButtonGroupItem::new("Archive").icon("archive"))
                .child(ButtonGroupItem::new("Delete").icon("trash")),
        )
        .child(
            Grid::new()
                .columns(3)
                .alignment(GridAlignment::Stretch)
                .gap(px(8.0))
                .child(
                    GridSpan::new().columns(2).child(
                        div()
                            .p(px(10.0))
                            .rounded(theme.tokens.radius_md)
                            .bg(theme.tokens.accent)
                            .child(label("GridSpan columns=2".to_string())),
                    ),
                )
                .child(
                    div()
                        .p(px(10.0))
                        .rounded(theme.tokens.radius_md)
                        .bg(theme.tokens.muted)
                        .child(label("Cell".to_string())),
                ),
        )
        .child(
            div()
                .h(px(48.0))
                .child(ResizeHandle::new(Axis::Horizontal).active(true)),
        )
        .child(div().w(px(280.0)).child(Slider::new(self.slider.clone())))
        .child(
            div().w(px(420.0)).child(
                Toolbar::new()
                    .label("View controls")
                    .size(ToolbarSize::Sm)
                    .start_content(
                        row()
                            .gap(px(4.0))
                            .child(Button::new("tb-new", "New").size(ButtonSize::Sm))
                            .child(
                                Button::new("tb-share", "Share")
                                    .size(ButtonSize::Sm)
                                    .variant(ButtonVariant::Ghost),
                            ),
                    )
                    .center_content(label("June 2026".to_string()))
                    .end_content(
                        row()
                            .gap(px(4.0))
                            .child(IconButton::new("chevron-left").size(px(28.0)))
                            .child(IconButton::new("chevron-right").size(px(28.0))),
                    ),
            ),
        );

        let more_inputs =
            section("More inputs", "Free-text field and icon buttons", &theme)
                .child(col().child(label_chip("Description", &theme)).child(
                    div().w(px(320.0)).child(
                        Input::new(&self.field_textarea).placeholder("Write a description..."),
                    ),
                ))
                .child(
                    row()
                        .child(IconButton::new("search"))
                        .child(IconButton::new("settings"))
                        .child(IconButton::new("plus")),
                );

        let nav_sec = section("Tabs", "Underline, enclosed and pill variants", &theme)
            .child(
                Tabs::new()
                    .variant(TabVariant::Underline)
                    .size(TabsSize::Sm)
                    .layout(TabsLayout::Fill)
                    .tabs(vec![
                        Tab::new("home", "Home"),
                        Tab::new("projects", "Projects"),
                        Tab::new("settings", "Settings"),
                    ])
                    .selected_index(0),
            )
            .child(
                Tabs::new()
                    .variant(TabVariant::Pills)
                    .size(TabsSize::Md)
                    .layout(TabsLayout::Hug)
                    .tabs(vec![
                        Tab::new("day", "Day"),
                        Tab::new("week", "Week"),
                        Tab::new("month", "Month"),
                    ])
                    .selected_index(1),
            );
        let nav_sec = nav_sec.child(
            TabList::new()
                .size(TabListSize::Sm)
                .layout(TabListLayout::Hug)
                .variant(TabVariant::Enclosed)
                .tab("inbox", "Inbox")
                .tab("sent", "Sent")
                .tab("archive", "Archive")
                .selected_id("inbox"),
        );

        let data_table = section("Data table", "Headers, rows and dividers", &theme)
            .child(
                Table::new()
                    .columns(vec![
                        TableColumn::new("Name").column_width(proportional(1.0)),
                        TableColumn::new("Role").column_width(pixel(px(110.0))),
                        TableColumn::new("Status")
                            .width(px(110.0))
                            .align(TableColumnAlign::Center),
                    ])
                    .vertical_align(TableVerticalAlign::Middle)
                    .rows(vec![
                        TableRow::new(vec!["Augustus Otu".into(), "Owner".into(), "Active".into()]),
                        TableRow::new(vec!["Kael UI".into(), "Editor".into(), "Active".into()]),
                        TableRow::new(vec!["Astryx".into(), "Viewer".into(), "Invited".into()]),
                    ]),
            )
            .child(
                Table::new()
                    .dividers(TableDividers::Grid)
                    .child(
                        TableHeader::new().child(
                            TableRow::children()
                                .header(true)
                                .cell(
                                    TableHeaderCell::new("Component")
                                        .width(px(160.0))
                                        .scope("col"),
                                )
                                .cell(TableHeaderCell::new("State").width(px(120.0)))
                                .cell(
                                    TableHeaderCell::new("Owner")
                                        .width(px(140.0))
                                        .align(TableColumnAlign::End),
                                ),
                        ),
                    )
                    .child(
                        TableBody::new()
                            .child(
                                TableRow::children()
                                    .cell(TableCell::new("CommandPalette").width(px(160.0)))
                                    .cell(TableCell::new(Badge::new("Done")).width(px(120.0)))
                                    .cell(
                                        TableCell::new("Kael")
                                            .width(px(140.0))
                                            .align(TableColumnAlign::End),
                                    ),
                            )
                            .child(
                                TableRow::children()
                                    .cell(TableCell::new("Table").width(px(160.0)))
                                    .cell(TableCell::new(Badge::new("Parity")).width(px(120.0)))
                                    .cell(
                                        TableCell::new("Astryx")
                                            .width(px(140.0))
                                            .align(TableColumnAlign::End),
                                    ),
                            ),
                    )
                    .child(
                        TableFooter::new().child(
                            TableRow::children()
                                .cell(TableCell::new("2 components").width(px(160.0)))
                                .cell(TableCell::new("Verified").width(px(120.0)))
                                .cell(
                                    TableCell::new("Showcase")
                                        .width(px(140.0))
                                        .align(TableColumnAlign::End),
                                ),
                        ),
                    ),
            );

        let parity_surfaces = section(
            "Astryx parity surfaces",
            "Newly covered component names and dense states",
            &theme,
        )
        .child(
            TopNav::new()
                .brand("Kael Console")
                .leading_icon("sparkles")
                .item(
                    TopNavItem::new("Overview")
                        .icon("layout-dashboard")
                        .selected(true),
                )
                .item(TopNavItem::new("Reports").icon("bar-chart-3"))
                .trailing(Button::new("top-new", "New").size(ButtonSize::Sm)),
        )
        .child(TopNavHeading::new("Kael UI").superheading("ASTRYX parity").logo("sparkles"))
        .child(
            Layout::new()
                .header(LayoutHeader::new(
                    div()
                        .child(
                            Heading::new("Layout frame")
                                .level(HeadingLevel::H4)
                                .heading_type(HeadingType::Display3),
                        ),
                ).has_divider(true))
                .panel(LayoutPanel::new(
                    div()
                        .p(px(8.0))
                        .child(
                            SideNav::new()
                                .items(vec![
                                    SideNavItem::new("home".into(), "Home").with_icon("home"),
                                    SideNavItem::new("teams".into(), "Teams").with_icon("users"),
                                    SideNavItem::new("billing".into(), "Billing")
                                        .with_icon("credit-card"),
                                ])
                                .selected_id("home"),
                        ),
                ).width(px(180.0)).has_divider(true))
                .content(LayoutContent::new(
                    div()
                        .child(
                            row()
                                .gap(px(8.0))
                                .items_center()
                                .child(MobileNavToggle::new().open(true).label("Toggle navigation"))
                                .child(
                                    SideNavCollapseButton::new()
                                        .collapsed(false)
                                        .label("Collapse sidebar"),
                                ),
                        )
                        .child(
                            AppShell::new()
                                .variant(AppShellVariant::Section)
                                .top(
                                    MobileNav::new("Mobile shell")
                                        .open(true)
                                        .item(NavItem::new("Inbox").icon("inbox").selected(true))
                                        .item(NavItem::new("Archive").icon("archive")),
                                )
                                .content(
                                    div()
                                        .p(px(12.0))
                                        .child(InteractiveRoleContext::new(
                                            InteractiveRole::Button,
                                        )
                                        .child(Item::new("Focusable action").icon("mouse-pointer")))
                                        .child(
                                            MetadataList::new()
                                                .columns(MetadataListColumns::Multi)
                                                .item(
                                                    MetadataListItem::new(
                                                        "Owner",
                                                        body("Design Systems".to_string()),
                                                    )
                                                    .icon("user"),
                                                )
                                                .item(
                                                    MetadataListItem::new(
                                                        "Status",
                                                        Badge::new("Aligned").hue(Hue::Green),
                                                    )
                                                    .icon("check-circle"),
                                                ),
                                        ),
                                ),
                        ),
                ).padding(px(12.0)))
                .gap(px(0.0)),
        )
        .child(
            Stack::new()
                .horizontal()
                .gap(px(8.0))
                .child(StackItem::new(Badge::new("static").hue(Hue::Gray)))
                .child(
                    StackItem::new(
                        div()
                            .h(px(28.0))
                            .rounded(theme.tokens.radius_md)
                            .bg(theme.tokens.muted)
                            .px(px(10.0))
                            .flex()
                            .items_center()
                            .text_size(px(13.0))
                            .child("StackItem::Fill"),
                    )
                    .size(StackItemSize::Fill),
                )
                .child(StackItem::new(Button::new("stack-action", "Action").size(ButtonSize::Sm))),
        )
        .child(
            row()
                .items_start()
                .child(
                    div().w(px(360.0)).child(
                        PowerSearch::new()
                            .config(
                                PowerSearchConfig::new("components")
                                    .field(
                                        PowerSearchField::new("status", "Status")
                                            .operator(PowerSearchOperator::new("is", "is"))
                                            .default_operator("is")
                                            .icon("success"),
                                    )
                                    .field(
                                        PowerSearchField::new("owner", "Owner")
                                            .operator(PowerSearchOperator::new(
                                                "contains",
                                                "contains",
                                            ))
                                            .typeahead_alias("assignee"),
                                    ),
                            )
                            .size(PowerSearchSize::Lg)
                            .label("Component filters")
                            .label_hidden(false)
                            .clearable(true)
                            .filters(vec![
                                PowerSearchFilter::new("status", "is", "active"),
                                PowerSearchFilter::new("owner", "contains", "design"),
                            ])
                            .query("release"),
                    ),
                )
                .child(
                    div().w(px(360.0)).child(
                        Tokenizer::new("Assignees")
                            .size(ControlSize::Lg)
                            .start_icon("users")
                            .token_overflow_behavior(TokenizerOverflowBehavior::UnfocusedInline)
                            .max_entries(5)
                            .entries_on_focus(true)
                            .max_menu_items(6)
                            .empty_search_results_text("No assignees found")
                            .creatable(true)
                            .value(vec![
                                TokenizerItem::new("ao", "Augustus"),
                                TokenizerItem::new("kael", "Kael UI"),
                                TokenizerItem::new("meta", "Meta"),
                                TokenizerItem::new("qa", "QA"),
                            ])
                            .max_visible(2)
                            .clearable(true)
                            .query("des"),
                    ),
                )
                .child(
                    div().w(px(360.0)).child(
                        Typeahead::new("Component")
                            .search_source(create_static_source(vec![
                                SearchableItem::new("button", "Button"),
                                SearchableItem::new("tokenizer", "Tokenizer"),
                                SearchableItem::new("power-search", "PowerSearch"),
                            ]))
                            .value(TypeaheadItem::new("Tokenizer", "tokenizer"))
                            .query("to")
                            .size(ControlSize::Md)
                            .start_icon("search")
                            .entries_on_focus(true)
                            .max_menu_items(5)
                            .empty_search_results_text("No components found")
                            .placeholder("Search components...")
                            .on_change(|_, _, _| {}),
                    ),
                ),
        )
        .child(
            row()
                .items_start()
                .child(
                    div().w(px(360.0)).child(
                        Table::new()
                            .density(TableDensity::Compact)
                            .dividers(TableDividers::Grid)
                            .striped(true)
                            .hover(true)
                            .text_overflow(TableTextOverflow::Truncate)
                            .columns(vec![
                                TableColumn::new("Component").width(px(130.0)),
                                TableColumn::new("State").width(px(100.0)),
                                TableColumn::new("Notes").width(px(220.0)),
                            ])
                            .rows(vec![
                                TableRow::new(vec![
                                    "Table".into(),
                                    "Updated".into(),
                                    "Density, dividers and truncation are visible".into(),
                                ])
                                .selected(true),
                                TableRow::new(vec![
                                    "TreeList".into(),
                                    "Updated".into(),
                                    "Header, density and row color parity".into(),
                                ]),
                            ]),
                    ),
                )
                .child(
                    div().w(px(360.0)).child(
                        TreeList::new()
                            .density(TreeListDensity::Compact)
                            .header(label("Repository outline".to_string()))
                            .expanded_ids(vec![SharedString::from("src")])
                            .selected_id(SharedString::from("components"))
                            .nodes(vec![TreeNode::new(SharedString::from("src"), "src")
                                .with_icon("folder")
                                .with_children(vec![
                                    TreeNode::new(SharedString::from("components"), "components")
                                        .with_icon("folder-open"),
                                    TreeNode::new(SharedString::from("theme"), "theme")
                                        .with_icon("palette"),
                                ])]),
                    ),
                ),
        )
        .child(
            row()
                .items_start()
                .child(
                    div().w(px(360.0)).child(
                        InputGroup::new("Repository slug")
                            .child(InputGroupText::new("github.com/"))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .h(px(32.0))
                                    .px(px(10.0))
                                    .border_y_1()
                                    .border_color(theme.tokens.input)
                                    .text_size(px(14.0))
                                    .text_color(theme.tokens.foreground)
                                    .child("facebook/astryx"),
                            )
                            .child(InputGroupText::new(".git")),
                    ),
                )
                .child(
                    div().w(px(360.0)).child(
                        CheckboxList::new().items(vec![
                            CheckboxListItem::new("api", "API parity")
                                .description("Named ASTRYX surfaces are exported")
                                .checked(true),
                            CheckboxListItem::new("visual", "Visual review")
                                .description("Rendered comparison still required"),
                        ]),
                    ),
                )
                .child(
                    div()
                        .w(px(220.0))
                        .child(Divider::new().variant(DividerVariant::Strong).label("strong")),
                )
        )
        .child(
            CollapsibleGroup::new()
                .child(
                    Collapsible::new()
                        .trigger(body("ASTRYX API names".to_string()))
                        .content(muted(
                            "Divider, InputGroupText and CollapsibleGroup are now public.",
                        ))
                        .open(true),
                )
                .child(SideNavHeading::new("Navigation aliases"))
                .child(
                    Collapsible::new()
                        .trigger(body("Visual details".to_string()))
                        .content(muted("Grouped rows share border and radius treatment.")),
                ),
        )
        .child(
            div()
                .w(px(420.0))
                .flex()
                .flex_col()
                .bg(theme.tokens.card)
                .border_1()
                .border_color(theme.tokens.border)
                .rounded(theme.tokens.radius_lg)
                .overflow_hidden()
                .child(
                    CommandPaletteInput::new(self.command_input.clone())
                        .placeholder("Search commands...")
                        .end_content(KBD::new("mod+k"))
                        .busy(false),
                )
                .child(
                    CommandPaletteList::new().child(
                        CommandPaletteGroup::new("Suggestions")
                            .child(
                                CommandPaletteItem::new("new-file", "New file")
                                    .description("Create a new component file")
                                    .icon("file-plus")
                                    .shortcut("N"),
                            )
                            .child(
                                CommandPaletteItem::new("review", "Review parity")
                                    .description("Run the Astryx visual audit")
                                    .icon("scan")
                                    .shortcut("R"),
                            ),
                    ),
                )
                .child(CommandPaletteEmpty::new("No commands found").h(px(56.0)))
                .child(
                    CommandPaletteFooter::new().child(
                        row()
                            .gap(px(12.0))
                            .child(caption("↑↓ Navigate".to_string()))
                            .child(caption("↵ Select".to_string()))
                            .child(caption("Esc Close".to_string())),
                    ),
                ),
        )
        .child(
            row()
                .items_start()
                .child(
                    div().w(px(360.0)).child(
                        Outline::new().items(vec![
                            OutlineItem::new("inputs", "Inputs").active(true),
                            OutlineItem::new("tables", "Tables").level(1),
                            OutlineItem::new("navigation", "Navigation").level(1),
                            OutlineItem::new("overlays", "Overlays").level(1),
                        ]),
                    ),
                ),
        )
        .child(
            row()
                .items_start()
                .child(
                    div().w(px(360.0)).child(
                        List::new()
                            .variant(ListVariant::Bordered)
                            .list_style(ListStyle::Decimal)
                            .child(
                                Item::new("Create component")
                                    .icon("plus")
                                    .description("Scaffold a new Astryx surface"),
                            )
                            .child(
                                Item::new("Review parity")
                                    .icon("scan")
                                    .description("Compare against upstream behavior")
                                    .selected(true),
                            ),
                    ),
                )
                .child(
                    div().w(px(360.0)).child(
                        OverflowList::new()
                            .max_visible(2)
                            .item(Badge::new("Button").hue(Hue::Blue))
                            .item(Badge::new("Table").hue(Hue::Green))
                            .item(Badge::new("TreeList").hue(Hue::Purple))
                            .item(Badge::new("Chat").hue(Hue::Pink)),
                    ),
                )
                .child(
                    row()
                        .gap(px(8.0))
                        .child(
                            Citation::new("facebook/astryx")
                                .number(1)
                                .source("GitHub")
                                .description("Reference design system for this parity pass."),
                        )
                        .child(
                            Citation::new("ASTRYX docs")
                                .number(2)
                                .variant(CitationVariant::Number),
                        ),
                    ),
        )
        .child(
            row()
                .items_start()
                .child(
                    div().w(px(420.0)).h(px(360.0)).child(
                        Chat::new()
                            .message(
                                ChatMessage::new("system", "Astryx parity audit started.")
                                    .role(ChatMessageRole::System)
                                    .timestamp("09:41"),
                            )
                            .message(
                                ChatMessage::new(
                                    "assistant",
                                    "Table, TreeList and tokenized search surfaces now render in the showcase.",
                                )
                                .author("Kael")
                                .timestamp("09:42"),
                            )
                            .message(
                                ChatMessage::new("user", "Run the visual QA pass next.")
                                    .role(ChatMessageRole::User)
                                    .author("You")
                                    .timestamp("09:43"),
                            )
                            .composer_value("Check desktop and mobile"),
                    ),
                )
                .child(
                    div()
                        .w(px(320.0))
                        .child(ClickableCard::new().selected(true).child(
                            col()
                                .child(Code::new("TableDensity::Compact").variant(CodeVariant::Inline))
                                .child(Link::new("Open upstream reference").external(true)),
                        )),
                ),
        );

        let timeline_sec = section("Timeline", "Activity feed", &theme).child(Timeline::new(vec![
            TimelineItem::new("Project created").description("Repository initialized"),
            TimelineItem::new("First release").description("v0.1.0 shipped"),
            TimelineItem::new("Astryx redesign").description("Components matched to Astryx"),
        ]));

        let empty_disclosure = section(
            "Empty state & collapsible",
            "Placeholders & disclosure",
            &theme,
        )
        .child(
            EmptyState::new("empty-demo", "No projects yet")
                .description("Create your first project to get started.")
                .icon("inbox"),
        )
        .child(
            Collapsible::new()
                .trigger(body("Advanced settings".to_string()))
                .content(muted(
                    "These options are hidden until expanded.".to_string(),
                ))
                .open(true),
        );

        let dropdowns = section(
            "Select & number input",
            "Dropdown and stepper input",
            &theme,
        )
        .child(div().w(px(280.0)).child(self.select.clone()))
        .child(
            div().w(px(180.0)).child(
                NumberInput::new(self.number.clone())
                    .start_icon("hash")
                    .units("items")
                    .status(FieldStatusType::Success),
            ),
        );

        let rating_stepper = section("Rating & stepper", "Feedback and multi-step flows", &theme)
            .child(Rating::new(self.rating.clone()))
            .child(Stepper::new(self.stepper.clone()));

        let otp_date = section("OTP & date picker", "Specialized inputs", &theme)
            .child(OTPInput::new(&self.otp))
            .child(
                div()
                    .w(px(220.0))
                    .child(DatePicker::new(self.date.clone()).size(InputSize::Lg)),
            )
            .child(
                Calendar::new()
                    .current_month(DateValue::new(2026, 6, 1))
                    .selected_date(DateValue::new(2026, 6, 26))
                    .min("2026-06-01")
                    .max("2026-07-31")
                    .has_week_numbers(true)
                    .week_starts_on(DayOfWeek::MONDAY),
            );

        let pickers = section("Pickers", "Time, color and file upload", &theme)
            .child(
                div()
                    .w(px(200.0))
                    .child(TimePicker::new(self.time_state.clone()).size(InputSize::Lg)),
            )
            .child(ColorPicker::new("cp-demo", self.color_state.clone()))
            .child(
                div()
                    .w(px(340.0))
                    .child(FileUpload::new("fu-demo", self.file_state.clone())),
            );

        let overlays = section("Overlays", "Hover card and popover", &theme)
            .child(
                row()
                    .child(
                        HoverCard::new()
                            .placement(LayerPlacement::Above)
                            .alignment(LayerAlignment::Center)
                            .focusTrigger(HoverCardFocusTrigger::Auto)
                            .hasHoverIndication(HoverCardHoverIndication::Always)
                            .isDefaultOpen(true)
                            .trigger(
                                Button::new("hc", "Hover card").variant(ButtonVariant::Outline),
                            )
                            .content(
                                col()
                                    .gap(px(4.0))
                                    .child(body("Augustus Otu".to_string()))
                                    .child(muted("Building the Kael UI framework".to_string())),
                            ),
                    )
                    .child(
                        Popover::new("pop")
                            .trigger(
                                Button::new("pop-t", "Open popover")
                                    .variant(ButtonVariant::Outline),
                            )
                            .content(|window, cx| {
                                cx.new(|cx| {
                                    PopoverContent::new(window, cx, |_w, _c| {
                                        col()
                                            .gap(px(6.0))
                                            .p(px(4.0))
                                            .child(body("Quick actions".to_string()))
                                            .child(muted("Rename · Duplicate · Delete".to_string()))
                                            .into_any_element()
                                    })
                                })
                            }),
                    ),
            )
            .child(
                div()
                    .relative()
                    .h(px(84.0))
                    .w(px(320.0))
                    .overflow_hidden()
                    .rounded(theme.tokens.radius_lg)
                    .border_1()
                    .border_color(theme.tokens.border)
                    .child(
                        Overlay::new()
                            .scrim(OverlayScrimMode::Light)
                            .position(OverlayPosition::Top)
                            .align(OverlayAlign::Start)
                            .content(
                                div()
                                    .m(px(8.0))
                                    .px(px(10.0))
                                    .py(px(6.0))
                                    .rounded(theme.tokens.radius_md)
                                    .bg(theme.tokens.card)
                                    .shadow(theme.tokens.shadow_sm.to_vec())
                                    .child(label("Overlay preview".to_string())),
                            ),
                    ),
            )
            .child(
                div()
                    .relative()
                    .h(px(178.0))
                    .w(px(320.0))
                    .overflow_hidden()
                    .rounded(theme.tokens.radius_lg)
                    .border_1()
                    .border_color(theme.tokens.border)
                    .child(
                        div()
                            .absolute()
                            .left(px(12.0))
                            .top(px(10.0))
                            .child(caption("ContextMenu data items".to_string())),
                    )
                    .child(
                        ContextMenu::new(point(px(14.0), px(38.0)))
                            .menu_width(px(188.0))
                            .item(ContextMenuItem::new("open", "Open").icon("external-link"))
                            .item(ContextMenuItem::new("copy", "Copy link").icon("copy"))
                            .item(ContextMenuItem::separator())
                            .item(
                                ContextMenuItem::new("delete", "Delete")
                                    .icon("trash")
                                    .destructive(true),
                            ),
                    ),
            );

        let modal_triggers = section(
            "Overlays — click to open",
            "Dialog, sheet and toast",
            &theme,
        )
        .child(
            row()
                .child(
                    Button::new("open-dialog", "Open dialog")
                        .variant(ButtonVariant::Outline)
                        .on_click({
                            let view = view.clone();
                            move |_, _, cx| {
                                view.update(cx, |this, cx| {
                                    this.show_dialog = true;
                                    cx.notify();
                                });
                            }
                        }),
                )
                .child(
                    Button::new("open-sheet", "Open sheet")
                        .variant(ButtonVariant::Outline)
                        .on_click({
                            let view = view.clone();
                            move |_, _, cx| {
                                view.update(cx, |this, cx| {
                                    this.show_sheet = true;
                                    cx.notify();
                                });
                            }
                        }),
                )
                .child(
                    Button::new("show-toast", "Show toast")
                        .variant(ButtonVariant::Outline)
                        .on_click({
                            let view = view.clone();
                            move |_, window, cx| {
                                view.update(cx, |this, cx| {
                                    this.toast_n += 1;
                                    let id = this.toast_n;
                                    this.toasts.update(cx, |manager, cx| {
                                        manager.add_toast(
                                            ToastItem::new(id, "Changes saved")
                                                .description("Your project was updated.")
                                                .variant(ToastVariant::Success),
                                            window,
                                            cx,
                                        );
                                    });
                                });
                            }
                        }),
                ),
        )
        .child(
            row()
                .items_start()
                .child(
                    Toast::new("Inline Toast mirrors the ASTRYX preview API.")
                        .toast_type(ToastType::Info)
                        .end_content(Badge::new("Info")),
                )
                .child(
                    Toast::new("Error toasts use assertive destructive styling.")
                        .toast_type(ToastType::Error)
                        .end_content(Badge::new("Error")),
                ),
        )
        .child(
            ToastViewport::new()
                .position(ToastPosition::BottomEnd)
                .max_visible(3)
                .child(label("ToastViewport wraps app content.".to_string())),
        );

        let code_tags = section("Code, tags & toggle group", "Tokens and snippets", &theme)
            .child(
                CodeBlock::new("let astryx = Theme::astryx_neutral();\ninstall_theme(cx, astryx);")
                    .language("rust"),
            )
            .child(
                row()
                    .child(Token::new("Default").icon("tag"))
                    .child(
                        Token::new("Green")
                            .color(TokenColor::Green)
                            .end_content(Badge::new("3")),
                    )
                    .child(
                        Token::new("Removable")
                            .color(TokenColor::Blue)
                            .on_remove(|_, _| {}),
                    )
                    .child(
                        Token::new("Hidden label")
                            .icon("sparkles")
                            .description("Accessible hidden label token")
                            .is_label_hidden(true),
                    )
                    .child(Token::new("Disabled").is_disabled(true)),
            )
            .child(TagInput::new(self.tags.clone()))
            .child(
                ToggleGroup::new()
                    .variant(ToggleGroupVariant::Single)
                    .items(vec![
                        ToggleGroupItem::new("bold", "Bold").icon("bold"),
                        ToggleGroupItem::new("italic", "Italic").icon("italic"),
                        ToggleGroupItem::new("underline", "Underline").icon("underline"),
                    ])
                    .value("bold"),
            );

        // ===== Actions =====
        let actions_icon = section(
            "Icon Buttons",
            "Icon-only actions across variants, sizes, and a copy-to-clipboard button",
            &theme,
        )
        .child(
            col().child(label_chip("Variants", &theme)).child(
                row()
                    .child(IconButton::new("star").variant(ButtonVariant::Default))
                    .child(IconButton::new("heart").variant(ButtonVariant::Secondary))
                    .child(IconButton::new("trash").variant(ButtonVariant::Destructive))
                    .child(IconButton::new("settings").variant(ButtonVariant::Outline))
                    .child(IconButton::new("search").variant(ButtonVariant::Ghost))
                    .child(IconButton::new("lock").disabled(true)),
            ),
        )
        .child(
            col().child(label_chip("Sizes", &theme)).child(
                row()
                    .child(
                        IconButton::new("plus")
                            .size(px(28.0))
                            .variant(ButtonVariant::Outline),
                    )
                    .child(
                        IconButton::new("plus")
                            .size(px(32.0))
                            .variant(ButtonVariant::Outline),
                    )
                    .child(
                        IconButton::new("plus")
                            .size(px(40.0))
                            .variant(ButtonVariant::Outline),
                    ),
            ),
        )
        .child(
            col()
                .child(label_chip("Copy button", &theme))
                .child(row().child(CopyButton::new("actions-copy", self.actions_copy.clone()))),
        );

        let actions_grouped = section(
            "Grouped & Overflow",
            "Segmented button groups and an overflow menu of actions",
            &theme,
        )
        .child(
            col().child(label_chip("Horizontal group", &theme)).child(
                ButtonGroup::new("actions-bg-h")
                    .size(ControlSize::Md)
                    .child(ButtonGroupItem::new("Bold").icon("bold"))
                    .child(ButtonGroupItem::new("Italic").icon("italic"))
                    .child(ButtonGroupItem::new("Underline").icon("underline")),
            ),
        )
        .child(
            col().child(label_chip("Vertical group", &theme)).child(
                ButtonGroup::new("actions-bg-v")
                    .orientation(ButtonGroupOrientation::Vertical)
                    .size(ControlSize::Sm)
                    .child(ButtonGroupItem::new("Preview").icon("eye"))
                    .child(ButtonGroupItem::new("Archive").icon("archive"))
                    .child(ButtonGroupItem::new("Delete").icon("trash")),
            ),
        )
        .child(
            col()
                .child(label_chip("More menu", &theme))
                .child(MoreMenu::new(vec![
                    MenuItem::new("rename", "Rename").with_icon("pencil"),
                    MenuItem::new("duplicate", "Duplicate").with_icon("copy"),
                    MenuItem::separator(),
                    MenuItem::new("delete", "Delete").with_icon("trash"),
                ])),
        );

        let actions_fab = section(
            "Floating Action Button",
            "Expandable FAB revealing staggered action items",
            &theme,
        )
        .child(
            div().relative().h(px(220.0)).w_full().child(
                div().absolute().bottom(px(8.0)).left(px(8.0)).child(
                    FloatingActionButton::new("actions-fab", self.actions_fab.clone())
                        .icon("+")
                        .size(FABSize::Md)
                        .action("compose", "✎", |_, _| {})
                        .action("upload", "↑", |_, _| {})
                        .action("share", "→", |_, _| {}),
                ),
            ),
        );

        // ===== Inputs =====
        let inputs_combobox_searchinput = section(
            "Combobox & search",
            "Searchable dropdown and a specialized search field with toggles",
            &theme,
        )
        .child(
            col()
                .child(label_chip("Combobox (filter as you type)", &theme))
                .child(div().w(px(280.0)).child(self.inputs_combobox.clone())),
        )
        .child(
            col()
                .child(label_chip("SearchInput (Aa / .* toggles, clear)", &theme))
                .child(div().w(px(360.0)).child(self.inputs_search.clone())),
        );

        let inputs_range_slider = section(
            "Range slider",
            "Dual-thumb selection with value readouts",
            &theme,
        )
        .child(
            col().child(label_chip("Default (md)", &theme)).child(
                div()
                    .w(px(320.0))
                    .child(RangeSlider::new(self.inputs_range.clone()).show_values(true)),
            ),
        )
        .child(
            col().child(label_chip("Large", &theme)).child(
                div()
                    .w(px(320.0))
                    .child(RangeSlider::new(self.inputs_range.clone()).size(SliderSize::Lg)),
            ),
        )
        .child(
            col().child(label_chip("Disabled", &theme)).child(
                div()
                    .w(px(320.0))
                    .child(RangeSlider::new(self.inputs_range.clone()).disabled(true)),
            ),
        );

        let inputs_field_label = section(
            "Field, FieldLabel & Label",
            "Form scaffolding: labels, descriptions, and status feedback",
            &theme,
        )
        .child(
            col()
                .child(label_chip("Label (required + helper)", &theme))
                .child(
                    Label::new("Email address")
                        .required(true)
                        .helper_text("We'll never share your email"),
                ),
        )
        .child(
            col()
                .child(label_chip("FieldLabel (optional, with icon)", &theme))
                .child(
                    FieldLabel::new("Display name")
                        .description("Shown on your public profile")
                        .optional(true)
                        .icon("user"),
                ),
        )
        .child(
            col()
                .child(label_chip("Field wrapping a control + error", &theme))
                .child(
                    Field::new(
                        "Workspace",
                        div()
                            .h(px(32.0))
                            .px(px(8.0))
                            .flex()
                            .items_center()
                            .bg(theme.tokens.card)
                            .border_1()
                            .border_color(theme.tokens.input)
                            .rounded(theme.tokens.radius_md)
                            .text_color(theme.tokens.muted_foreground)
                            .child("acme-team"),
                    )
                    .description("Letters, numbers, and dashes")
                    .required(true)
                    .status(FieldStatusType::Error, "This workspace is already taken")
                    .width(px(320.0)),
                ),
        );

        let inputs_field_status = section(
            "Field status",
            "Standalone validation messages across tones and variants",
            &theme,
        )
        .child(
            col().child(label_chip("Tones (attached)", &theme)).child(
                col()
                    .w(px(320.0))
                    .child(FieldStatus::info("Heads up, double check this value").show_icon(true))
                    .child(FieldStatus::success("Looks good!").show_icon(true))
                    .child(FieldStatus::warning("This may need attention").show_icon(true))
                    .child(FieldStatus::error("Something went wrong").show_icon(true)),
            ),
        )
        .child(
            col().child(label_chip("Detached variant", &theme)).child(
                div().w(px(320.0)).child(
                    FieldStatus::error("Detached message with spacing")
                        .detached()
                        .show_icon(true),
                ),
            ),
        );

        let inputs_input_group =
            section("Input group", "Connected addons via InputGroupText", &theme).child(
                col().child(label_chip("Prefix + control", &theme)).child(
                    InputGroup::new("Website")
                        .description("Enter your site URL")
                        .child(InputGroupText::new("https://"))
                        .child(
                            div()
                                .flex_1()
                                .h(px(32.0))
                                .px(px(8.0))
                                .flex()
                                .items_center()
                                .bg(theme.tokens.card)
                                .border_1()
                                .border_color(theme.tokens.input)
                                .text_color(theme.tokens.muted_foreground)
                                .child("example.com"),
                        )
                        .child(InputGroupText::new(".dev"))
                        .w(px(360.0)),
                ),
            );

        let inputs_tokenizer_typeahead = section(
            "Tokenizer & typeahead",
            "Token input surface and a selected-token search shell",
            &theme,
        )
        .child(
            col()
                .child(label_chip("Tokenizer (value tokens, clearable)", &theme))
                .child(
                    div().w(px(360.0)).child(
                        Tokenizer::new("Recipients")
                            .description("Add people to share with")
                            .placeholder("Add a name...")
                            .clearable(true)
                            .start_icon("users")
                            .value(vec![
                                TokenizerItem::new("1", "Ada Lovelace"),
                                TokenizerItem::new("2", "Alan Turing"),
                            ]),
                    ),
                ),
        )
        .child(
            col()
                .child(label_chip("Typeahead (selected token + search)", &theme))
                .child(
                    div().w(px(360.0)).child(
                        Typeahead::new("Assignee")
                            .placeholder("Search teammates...")
                            .search_source(SearchSource::new([
                                SearchableItem::new("ada", "Ada Lovelace"),
                                SearchableItem::new("alan", "Alan Turing"),
                                SearchableItem::new("grace", "Grace Hopper"),
                            ]))
                            .value(TypeaheadItem::new("Grace Hopper", "grace"))
                            .clearable(true),
                    ),
                ),
        );

        // ===== Selection =====
        let selection_toggle_group = {
            let view_single = view.clone();
            let view_multi = view.clone();
            section(
                "Toggle group",
                "Single and multi-select grouped toggles",
                &theme,
            )
            .child(
                col()
                    .child(label_chip("Single (text formatting)", &theme))
                    .child(
                        ToggleGroup::new()
                            .variant(ToggleGroupVariant::Single)
                            .size(ToggleGroupSize::Md)
                            .items(vec![
                                ToggleGroupItem::new("bold", "Bold"),
                                ToggleGroupItem::new("italic", "Italic"),
                                ToggleGroupItem::new("underline", "Underline"),
                            ])
                            .value(self.selection_toggle_value.clone())
                            .on_change(move |value, _, cx| {
                                let next = value.clone();
                                view_single.update(cx, |this, cx| {
                                    this.selection_toggle_value = next;
                                    cx.notify();
                                });
                            }),
                    )
                    .child(label_chip("Multiple (view options)", &theme))
                    .child(
                        ToggleGroup::new()
                            .variant(ToggleGroupVariant::Multiple)
                            .size(ToggleGroupSize::Sm)
                            .items(vec![
                                ToggleGroupItem::new("grid", "Grid"),
                                ToggleGroupItem::new("list", "List"),
                                ToggleGroupItem::new("compact", "Compact").disabled(true),
                            ])
                            .values(self.selection_toggle_views.clone())
                            .on_multiple_change(move |values, _, cx| {
                                let next = values.clone();
                                view_multi.update(cx, |this, cx| {
                                    this.selection_toggle_views = next;
                                    cx.notify();
                                });
                            }),
                    ),
            )
        };

        let selection_checkbox_list = {
            let view_checks = view.clone();
            section(
                "Checkbox list",
                "Grouped checkbox rows with descriptions",
                &theme,
            )
            .child(
                CheckboxList::new()
                    .label("Email preferences")
                    .description("Choose which updates to receive")
                    .density(ListDensity::Balanced)
                    .size(CheckboxSize::Md)
                    .has_dividers(true)
                    .item(
                        CheckboxListItem::new("analytics", "Weekly analytics")
                            .description("Performance digest every Monday")
                            .end_content("Recommended"),
                    )
                    .item(
                        CheckboxListItem::new("updates", "Product updates")
                            .description("New features and changelog"),
                    )
                    .item(
                        CheckboxListItem::new("marketing", "Marketing")
                            .description("Promotions and offers")
                            .disabled(true),
                    )
                    .value(self.selection_checks.clone())
                    .on_change(move |id, checked, _, cx| {
                        let id = id.clone();
                        view_checks.update(cx, |this, cx| {
                            this.selection_checks.retain(|existing| existing != &id);
                            if checked {
                                this.selection_checks.push(id);
                            }
                            cx.notify();
                        });
                    }),
            )
        };

        let selection_multi_selector = section(
            "Multi selector",
            "Trigger with token badges and overflow",
            &theme,
        )
        .child(
            col()
                .child(
                    MultiSelector::new("Assignees")
                        .description("Pick teammates for this task")
                        .placeholder("Select assignees")
                        .start_icon("users")
                        .clearable(true)
                        .max_visible(2)
                        .options(vec![
                            MultiSelectorOption::new("Ada Lovelace", "ada").selected(true),
                            MultiSelectorOption::new("Alan Turing", "alan").selected(true),
                            MultiSelectorOption::new("Grace Hopper", "grace").selected(true),
                            MultiSelectorOption::new("Edsger Dijkstra", "edsger"),
                        ]),
                )
                .child(label_chip("Empty + small", &theme))
                .child(
                    MultiSelector::new("Labels")
                        .size(InputSize::Sm)
                        .hidden_label(true)
                        .placeholder("Add labels")
                        .options(vec![
                            MultiSelectorOption::new("Bug", "bug"),
                            MultiSelectorOption::new("Feature", "feature"),
                        ]),
                ),
        );

        let selection_animated_switch = {
            let view_switch = view.clone();
            let active = self.selection_switch_active;
            let next = (active + 1) % 3;
            section(
                "Animated switch",
                "Cross-fade between mutually exclusive views",
                &theme,
            )
            .child(
                col()
                    .child(
                        div().h(px(72.0)).child(
                            AnimatedSwitch::new("selection-animated-switch")
                                .active(active)
                                .transition(AnimatedSwitchTransition::Fade)
                                .duration(std::time::Duration::from_millis(280))
                                .child(0, body("Overview panel"))
                                .child(1, body("Activity panel"))
                                .child(2, body("Settings panel")),
                        ),
                    )
                    .child(
                        Button::new("selection-switch-next", "Next view")
                            .variant(ButtonVariant::Secondary)
                            .size(ButtonSize::Sm)
                            .on_click(move |_, _, cx| {
                                view_switch.update(cx, |this, cx| {
                                    this.selection_switch_active = next;
                                    cx.notify();
                                });
                            }),
                    ),
            )
        };

        let selection_dropdown = section(
            "Dropdown menu",
            "Trigger-anchored menu with icons and separators",
            &theme,
        )
        .child(
            Dropdown::new(
                self.selection_dropdown.clone(),
                Button::new("selection-dropdown-trigger", "Actions")
                    .variant(ButtonVariant::Outline)
                    .size(ButtonSize::Md),
            )
            .align(DropdownAlign::Start)
            .min_width(px(200.0))
            .items(vec![
                DropdownItem::new("edit", "Edit")
                    .icon("pencil")
                    .shortcut("E"),
                DropdownItem::new("duplicate", "Duplicate")
                    .icon("copy")
                    .description("Create a copy"),
                DropdownItem::separator(),
                DropdownItem::new("archive", "Archive")
                    .icon("archive")
                    .disabled(true),
                DropdownItem::new("delete", "Delete")
                    .icon("trash-2")
                    .destructive(true)
                    .shortcut("Del"),
            ]),
        );

        // ===== DataDisplay =====
        let dd_lists = section(
            "Lists & items",
            "Grouped rows, list variants and accordion",
            &theme,
        )
        .child(
            col()
                .child(label_chip("List + Item (bordered, dividers)", &theme))
                .child(
                    List::new()
                        .variant(ListVariant::Bordered)
                        .density(ListDensity::Balanced)
                        .has_dividers(true)
                        .child(
                            Item::new("Profile")
                                .description("Manage your public details")
                                .icon("user")
                                .end_content(Icon::new("chevron-right").size(px(16.0))),
                        )
                        .child(
                            Item::new("Notifications")
                                .description("Email and push preferences")
                                .icon("bell")
                                .selected(true),
                        )
                        .child(
                            Item::new("Billing")
                                .description("Plans and invoices")
                                .icon("creditCard")
                                .disabled(true),
                        ),
                ),
        )
        .child(
            col()
                .child(label_chip("List styles (decimal / disc)", &theme))
                .child(
                    row()
                        .child(
                            List::new()
                                .list_style(ListStyle::Decimal)
                                .child(body("Install the kael crate"))
                                .child(body("Wire the prelude"))
                                .child(body("Ship a native app")),
                        )
                        .child(
                            List::new()
                                .list_style(ListStyle::Disc)
                                .child(body("Metal renderer"))
                                .child(body("Astryx tokens"))
                                .child(body("Accessibility")),
                        ),
                ),
        )
        .child(
            col()
                .child(label_chip("Accordion (single open)", &theme))
                .child(
                    Accordion::new("dd-accordion")
                        .bordered(true)
                        .item(|item| {
                            item.title("What is Kael?")
                                .icon("info")
                                .content(body("Kael is a native Rust GPUI app framework."))
                                .open(true)
                        })
                        .item(|item| {
                            item.title("Is it production ready?")
                                .icon("shield")
                                .content(body("Core rendering and components are stable."))
                        })
                        .item(|item| {
                            item.title("Which platforms?")
                                .icon("monitor")
                                .content(body("macOS, Windows and Linux."))
                        }),
                ),
        );

        let dd_tree = section(
            "Tree & metadata",
            "Hierarchical tree and label/value metadata",
            &theme,
        )
        .child(
            col().child(label_chip("TreeList", &theme)).child(
                TreeList::new()
                    .density(TreeListDensity::Balanced)
                    .selected_id("readme")
                    .expanded_ids(vec!["src", "components"])
                    .nodes(vec![
                        TreeNode::new("src", "src")
                            .with_icon("folder")
                            .with_children(vec![
                                TreeNode::new("components", "components")
                                    .with_icon("folder")
                                    .with_children(vec![
                                        TreeNode::new("button", "button.rs").with_icon("file"),
                                        TreeNode::new("list", "list.rs").with_icon("file"),
                                    ]),
                                TreeNode::new("lib", "lib.rs").with_icon("file"),
                            ]),
                        TreeNode::new("readme", "README.md").with_icon("file"),
                    ]),
            ),
        )
        .child(
            col()
                .child(label_chip("MetadataList (two columns)", &theme))
                .child(
                    MetadataList::new()
                        .columns(MetadataListColumns::Count(2))
                        .item(MetadataListItem::new("Status", body("Active")).icon("circleCheck"))
                        .item(MetadataListItem::new("Owner", body("Augustus Otu")).icon("user"))
                        .item(MetadataListItem::new("Region", body("us-east-1")).icon("globe"))
                        .item(MetadataListItem::new("Version", code("v0.2.0")).icon("tag")),
                ),
        );

        let dd_keys = section(
            "Keyboard & QR",
            "Shortcut keys, kbd chips and QR codes",
            &theme,
        )
        .child(
            col()
                .child(label_chip("Kbd (sizes & combos)", &theme))
                .child(
                    row()
                        .child(KBD::new("cmd+k").size(KBDSize::Sm))
                        .child(KBD::new("cmd+shift+p").size(KBDSize::Md))
                        .child(KBD::new("ctrl+enter").size(KBDSize::Lg)),
                ),
        )
        .child(
            col()
                .child(label_chip("KeyboardShortcuts", &theme))
                .child(self.dd_shortcuts.clone()),
        )
        .child(
            col().child(label_chip("QRCodeComponent", &theme)).child(
                row()
                    .child(QRCodeComponent::new("https://github.com/kael").size(px(120.0)))
                    .child(
                        QRCodeComponent::new("kael://launch")
                            .size(px(120.0))
                            .fg_color(theme.tokens.primary),
                    ),
            ),
        );

        let dd_misc = section(
            "Expandable & timestamps",
            "Expandable card and relative/absolute times",
            &theme,
        )
        .child(
            col()
                .child(label_chip("ExpandableCard (click to toggle)", &theme))
                .child(
                    ExpandableCard::new("dd-expandable", self.dd_expandable.clone())
                        .collapsed(
                            row()
                                .child(Icon::new("info").size(px(18.0)))
                                .child(body("Release notes — click to expand")),
                        )
                        .expanded(
                            col()
                                .child(h5("Release 0.2.0"))
                                .child(body(
                                    "Custom themes, live switching and self-sufficient prelude.",
                                ))
                                .child(muted("Click again to collapse.")),
                        )
                        .w(px(360.0)),
                ),
        )
        .child(
            col()
                .child(label_chip("Timestamp (formats)", &theme))
                .child(
                    row()
                        .child(Timestamp::new(1_718_000_000_i64).format(TimestampFormat::Relative))
                        .child(Timestamp::new(1_718_000_000_i64).format(TimestampFormat::Date))
                        .child(Timestamp::new(1_718_000_000_i64).format(TimestampFormat::DateTime))
                        .child(
                            Timestamp::new(1_718_000_000_i64)
                                .format(TimestampFormat::SystemDateTime),
                        ),
                ),
        );

        // ===== Charts =====
        let charts_bar = section(
            "Bar Chart",
            "Vertical, horizontal, and grouped multi-series bars",
            &theme,
        )
        .child(
            col()
                .child(label_chip(
                    "Vertical (single series, values + grid)",
                    &theme,
                ))
                .child(
                    BarChart::new(vec![
                        BarChartData::new("Jan", 120.0),
                        BarChartData::new("Feb", 200.0),
                        BarChartData::new("Mar", 150.0),
                        BarChartData::new("Apr", 280.0),
                        BarChartData::new("May", 190.0),
                    ])
                    .show_values(true)
                    .show_grid(true)
                    .chart_height(px(200.0)),
                ),
        )
        .child(
            col().child(label_chip("Horizontal", &theme)).child(
                div().w(px(360.0)).child(
                    BarChart::new(vec![
                        BarChartData::new("Rust", 92.0),
                        BarChartData::new("Swift", 74.0),
                        BarChartData::new("Go", 65.0),
                        BarChartData::new("TS", 58.0),
                    ])
                    .horizontal()
                    .show_values(true),
                ),
            ),
        )
        .child(
            col()
                .child(label_chip("Grouped (multi-series, legend)", &theme))
                .child(
                    BarChart::multi_series(
                        vec!["Q1", "Q2", "Q3", "Q4"],
                        vec![
                            BarChartSeries::new("Revenue", vec![120.0, 180.0, 150.0, 240.0]),
                            BarChartSeries::new("Cost", vec![80.0, 110.0, 95.0, 140.0]),
                        ],
                    )
                    .show_legend(true)
                    .chart_height(px(200.0)),
                ),
        );

        let charts_line = section(
            "Line Chart",
            "Single and multi-series lines, smoothing, filled area",
            &theme,
        )
        .child(
            col()
                .child(label_chip("Single series with points + x labels", &theme))
                .child(
                    div().w(px(420.0)).h(px(240.0)).child(
                        LineChart::single(
                            LineChartSeries::new(
                                "Visits",
                                vec![
                                    LineChartPoint::new(0.0, 30.0),
                                    LineChartPoint::new(1.0, 55.0),
                                    LineChartPoint::new(2.0, 42.0),
                                    LineChartPoint::new(3.0, 78.0),
                                    LineChartPoint::new(4.0, 64.0),
                                    LineChartPoint::new(5.0, 95.0),
                                ],
                            )
                            .show_points(true),
                        )
                        .x_labels(vec!["Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]),
                    ),
                ),
        )
        .child(
            col()
                .child(label_chip("Multi-series, smoothed, filled", &theme))
                .child(
                    div().w(px(420.0)).h(px(240.0)).child(
                        LineChart::new(vec![
                            LineChartSeries::new(
                                "Desktop",
                                vec![
                                    LineChartPoint::new(0.0, 40.0),
                                    LineChartPoint::new(1.0, 62.0),
                                    LineChartPoint::new(2.0, 50.0),
                                    LineChartPoint::new(3.0, 88.0),
                                    LineChartPoint::new(4.0, 72.0),
                                ],
                            )
                            .fill_area(true),
                            LineChartSeries::new(
                                "Mobile",
                                vec![
                                    LineChartPoint::new(0.0, 20.0),
                                    LineChartPoint::new(1.0, 35.0),
                                    LineChartPoint::new(2.0, 48.0),
                                    LineChartPoint::new(3.0, 44.0),
                                    LineChartPoint::new(4.0, 66.0),
                                ],
                            ),
                        ])
                        .smooth(true),
                    ),
                ),
        );

        let charts_area = section("Area Chart", "Overlaid and stacked filled areas", &theme)
            .child(
                col().child(label_chip("Overlaid (default)", &theme)).child(
                    AreaChart::new()
                        .size(AreaChartSize::Md)
                        .series(AreaChartSeries::new(
                            "2024",
                            vec![
                                (0.0, 30.0),
                                (1.0, 55.0),
                                (2.0, 48.0),
                                (3.0, 80.0),
                                (4.0, 70.0),
                            ],
                        ))
                        .series(AreaChartSeries::new(
                            "2025",
                            vec![
                                (0.0, 45.0),
                                (1.0, 40.0),
                                (2.0, 68.0),
                                (3.0, 60.0),
                                (4.0, 95.0),
                            ],
                        ))
                        .x_labels(vec!["Jan", "Feb", "Mar", "Apr", "May"]),
                ),
            )
            .child(
                col().child(label_chip("Stacked", &theme)).child(
                    AreaChart::new()
                        .size(AreaChartSize::Md)
                        .stacked()
                        .series(AreaChartSeries::new(
                            "Organic",
                            vec![
                                (0.0, 20.0),
                                (1.0, 30.0),
                                (2.0, 28.0),
                                (3.0, 40.0),
                                (4.0, 38.0),
                            ],
                        ))
                        .series(AreaChartSeries::new(
                            "Paid",
                            vec![
                                (0.0, 15.0),
                                (1.0, 18.0),
                                (2.0, 22.0),
                                (3.0, 20.0),
                                (4.0, 30.0),
                            ],
                        )),
                ),
            );

        let charts_pie_donut = section(
            "Pie & Donut",
            "Proportional segments with legends and center labels",
            &theme,
        )
        .child(
            row()
                .child(
                    col()
                        .child(label_chip("Pie with legend + percentages", &theme))
                        .child(
                            PieChart::pie(vec![
                                PieChartSegment::new("Chrome", 62.0),
                                PieChartSegment::new("Safari", 19.0),
                                PieChartSegment::new("Firefox", 11.0),
                                PieChartSegment::new("Edge", 8.0),
                            ])
                            .size(PieChartSize::Md)
                            .label_position(PieChartLabelPosition::Legend)
                            .show_percentages(true),
                        ),
                )
                .child(
                    col()
                        .child(label_chip("Donut with center value", &theme))
                        .child(
                            DonutChart::new()
                                .size(DonutChartSize::Md)
                                .segments(vec![
                                    PieChartSegment::new("Used", 68.0),
                                    PieChartSegment::new("Free", 32.0),
                                ])
                                .center_value("68%")
                                .center_label("Storage")
                                .show_legend(true)
                                .show_percentages(true),
                        ),
                ),
        );

        let charts_gauge = section("Gauge", "Semicircular progress indicators", &theme).child(
            row()
                .child(
                    col().child(label_chip("Small", &theme)).child(
                        Gauge::new("gauge-cpu")
                            .value(0.42)
                            .label("CPU")
                            .size(GaugeSize::Sm),
                    ),
                )
                .child(
                    col().child(label_chip("Medium", &theme)).child(
                        Gauge::new("gauge-mem")
                            .value(0.73)
                            .label("Memory")
                            .size(GaugeSize::Md),
                    ),
                )
                .child(
                    col()
                        .child(label_chip("Large, custom format", &theme))
                        .child(
                            Gauge::new("gauge-score")
                                .value(0.86)
                                .label("Score")
                                .size(GaugeSize::Lg)
                                .format(|v| format!("{:.1}", v * 10.0)),
                        ),
                ),
        );

        let charts_sparkline = section("Sparkline", "Compact inline trend mini-charts", &theme)
            .child(
                col()
                    .child(label_chip("Line, area, and bar variants", &theme))
                    .child(
                        row()
                            .child(
                                Sparkline::line(vec![4.0, 8.0, 5.0, 12.0, 9.0, 15.0, 11.0, 18.0])
                                    .size(SparklineSize::Lg),
                            )
                            .child(
                                Sparkline::area(vec![10.0, 7.0, 9.0, 6.0, 11.0, 8.0, 13.0])
                                    .size(SparklineSize::Lg),
                            )
                            .child(
                                Sparkline::bar(vec![3.0, 6.0, 4.0, 8.0, 5.0, 9.0, 7.0])
                                    .size(SparklineSize::Lg),
                            ),
                    ),
            )
            .child(
                col()
                    .child(label_chip("Trend coloring + min/max markers", &theme))
                    .child(
                        row()
                            .child(
                                Sparkline::line(vec![5.0, 8.0, 6.0, 11.0, 14.0, 19.0])
                                    .size(SparklineSize::Lg)
                                    .show_trend(true)
                                    .show_min_max(true),
                            )
                            .child(
                                Sparkline::line(vec![20.0, 16.0, 18.0, 12.0, 9.0, 5.0])
                                    .size(SparklineSize::Lg)
                                    .show_trend(true),
                            ),
                    ),
            );

        let charts_radar = section(
            "Radar Chart",
            "Multi-axis comparison (values normalized 0..1)",
            &theme,
        )
        .child(
            col()
                .child(label_chip("Two datasets across 5 axes", &theme))
                .child(
                    RadarChart::new()
                        .size(RadarChartSize::Md)
                        .axes(vec!["Speed", "Power", "Range", "Agility", "Defense"])
                        .dataset(RadarDataset::new("Model A", vec![0.8, 0.6, 0.9, 0.5, 0.7]))
                        .dataset(RadarDataset::new("Model B", vec![0.6, 0.9, 0.5, 0.8, 0.6])),
                ),
        );

        let charts_heatmap = section("Heatmap", "Color-graded matrix with axis labels", &theme)
            .child(
                col()
                    .child(label_chip("Activity by day and hour", &theme))
                    .child(
                        Heatmap::new()
                            .data(vec![
                                vec![2.0, 8.0, 14.0, 6.0, 3.0],
                                vec![5.0, 12.0, 18.0, 10.0, 4.0],
                                vec![1.0, 6.0, 22.0, 16.0, 9.0],
                            ])
                            .x_labels(vec!["6a", "9a", "12p", "3p", "6p"])
                            .y_labels(vec!["Mon", "Tue", "Wed"])
                            .cell_size(px(44.0))
                            .show_values(true),
                    ),
            );

        // ===== Feedback =====
        let fb_circular = section(
            "Circular Progress",
            "Determinate rings and indeterminate spinners",
            &theme,
        )
        .child(
            row()
                .child(
                    col()
                        .child(label_chip("0.25", &theme))
                        .child(CircularProgress::new(0.25)),
                )
                .child(
                    col()
                        .child(label_chip("0.75 success", &theme))
                        .child(CircularProgress::new(0.75).variant(ProgressVariant::Success)),
                )
                .child(
                    col().child(label_chip("growing", &theme)).child(
                        CircularProgress::new(0.6)
                            .spinner_type(SpinnerType::GrowingCircle)
                            .size(px(48.0)),
                    ),
                )
                .child(
                    col()
                        .child(label_chip("dot spinner", &theme))
                        .child(CircularProgress::indeterminate()),
                )
                .child(
                    col().child(label_chip("arc", &theme)).child(
                        CircularProgress::indeterminate()
                            .spinner_type(SpinnerType::Arc)
                            .variant(ProgressVariant::Accent),
                    ),
                )
                .child(
                    col().child(label_chip("arc no track", &theme)).child(
                        CircularProgress::indeterminate()
                            .spinner_type(SpinnerType::ArcNoTrack)
                            .size(px(56.0)),
                    ),
                ),
        );

        let fb_animated_progress = section(
            "Animated Progress",
            "Width tweens on value change, optional shimmer",
            &theme,
        )
        .child(
            col()
                .child(label_chip("40% default", &theme))
                .child(AnimatedProgress::new("fb-ap-1").value(0.4))
                .child(label_chip("70% success + shimmer", &theme))
                .child(
                    AnimatedProgress::new("fb-ap-2")
                        .value(0.7)
                        .variant(ProgressVariant::Success)
                        .shimmer(true),
                )
                .child(label_chip("90% warning, large", &theme))
                .child(
                    AnimatedProgress::new("fb-ap-3")
                        .value(0.9)
                        .variant(ProgressVariant::Warning)
                        .size(ProgressSize::Lg),
                ),
        );

        let fb_numbers = section(
            "Counters & Tickers",
            "Animated counter, countdown timer, and digit ticker",
            &theme,
        )
        .child(
            row()
                .child(
                    col().child(label_chip("animated counter", &theme)).child(
                        AnimatedCounter::new("fb-counter", self.fb_counter.clone())
                            .prefix("$")
                            .suffix(" MRR")
                            .text_size(px(28.0)),
                    ),
                )
                .child(
                    col().child(label_chip("number ticker", &theme)).child(
                        NumberTicker::new("fb-ticker", 1_482_390)
                            .separator(',')
                            .text_size(px(28.0)),
                    ),
                )
                .child(
                    col()
                        .child(label_chip("number ticker, suffix", &theme))
                        .child(
                            NumberTicker::new("fb-ticker-2", 98)
                                .suffix("%")
                                .text_size(px(28.0)),
                        ),
                ),
        )
        .child(
            col()
                .child(label_chip("countdown (d : h : m : s)", &theme))
                .child(
                    Countdown::new("fb-countdown", self.fb_countdown.clone())
                        .size(CountdownSize::Md)
                        .separator(CountdownSeparator::Colon),
                ),
        );

        let fb_indicators = section(
            "Indicators",
            "Pulse dots and notification bell with unread badge",
            &theme,
        )
        .child(
            row()
                .child(
                    col()
                        .child(label_chip("default", &theme))
                        .child(PulseIndicator::new("fb-pulse-1")),
                )
                .child(
                    col()
                        .child(label_chip("blue", &theme))
                        .child(PulseIndicator::new("fb-pulse-2").color(theme.tokens.primary)),
                )
                .child(
                    col().child(label_chip("alert, large, fast", &theme)).child(
                        PulseIndicator::new("fb-pulse-3")
                            .color(theme.tokens.destructive)
                            .size(px(12.0))
                            .speed(std::time::Duration::from_secs(1)),
                    ),
                )
                .child(
                    col()
                        .child(label_chip("notification bell", &theme))
                        .child(NotificationBell::new(self.fb_notifications.clone()).id("fb-bell")),
                ),
        );

        let fb_loading = section(
            "Loading Placeholders",
            "Shimmer sweep and skeleton loader",
            &theme,
        )
        .child(
            col()
                .child(label_chip("shimmer block", &theme))
                .child(
                    Shimmer::new()
                        .w(px(280.0))
                        .h(px(72.0))
                        .rounded(theme.tokens.radius_lg)
                        .bg(theme.tokens.muted),
                )
                .child(label_chip("skeleton loader (4 lines)", &theme))
                .child(
                    div().w(px(360.0)).child(
                        SkeletonLoader::new("fb-skeleton", self.fb_skeleton.clone())
                            .lines(4)
                            .line_height(px(14.0)),
                    ),
                ),
        );

        // ===== Navigation =====
        let nav_menus = section("Menus", "Dropdown menus and a menu bar", &theme)
            .child(
                col()
                    .child(label_chip("MenuBar", &theme))
                    .child(self.nav_menu_bar.clone()),
            )
            .child(
                col()
                    .child(label_chip("Menu", &theme))
                    .child(Menu::new(vec![
                        MenuItem::new("cut", "Cut")
                            .with_icon("scissors")
                            .with_shortcut("\u{2318}X"),
                        MenuItem::new("copy", "Copy")
                            .with_icon("copy")
                            .with_shortcut("\u{2318}C"),
                        MenuItem::new("paste", "Paste")
                            .with_icon("clipboard")
                            .with_shortcut("\u{2318}V"),
                        MenuItem::separator(),
                        MenuItem::checkbox("wrap", "Word Wrap", true),
                        MenuItem::submenu("share", "Share").with_icon("share-2"),
                        MenuItem::new("delete", "Delete")
                            .with_icon("trash-2")
                            .disabled(true),
                    ])),
            );

        let nav_toolbar = section(
            "Toolbar & NavItem",
            "Grouped icon buttons and nav rows",
            &theme,
        )
        .child(label_chip("Toolbar", &theme))
        .child(
            Toolbar::new()
                .size(ToolbarSize::Md)
                .group(
                    ToolbarGroup::new()
                        .button(ToolbarButton::new("undo", "undo").tooltip("Undo"))
                        .button(ToolbarButton::new("redo", "redo").tooltip("Redo")),
                )
                .group(
                    ToolbarGroup::new()
                        .button(
                            ToolbarButton::new("bold", "bold")
                                .variant(ToolbarButtonVariant::Toggle)
                                .pressed(true),
                        )
                        .button(
                            ToolbarButton::new("italic", "italic")
                                .variant(ToolbarButtonVariant::Toggle),
                        )
                        .button(
                            ToolbarButton::new("font", "type")
                                .variant(ToolbarButtonVariant::Dropdown),
                        )
                        .button(ToolbarButton::new("link", "link").disabled(true)),
                ),
        )
        .child(label_chip("NavItem", &theme))
        .child(
            col()
                .child(
                    NavItem::new("Dashboard")
                        .icon("layout-dashboard")
                        .selected(true),
                )
                .child(NavItem::new("Inbox").icon("inbox").badge("12"))
                .child(NavItem::new("Settings").icon("settings"))
                .child(NavItem::new("Archive").icon("archive").disabled(true)),
        );

        let nav_chrome = section(
            "App chrome",
            "File tree, status bar, top & side navigation",
            &theme,
        )
        .child(label_chip("FileTree", &theme))
        .child(
            FileTree::new()
                .nodes(vec![
                    FileNode::directory("src").with_children(vec![
                        FileNode::file("main.rs"),
                        FileNode::file("lib.rs"),
                        FileNode::directory("ui").with_children(vec![FileNode::file("button.rs")]),
                    ]),
                    FileNode::file("Cargo.toml"),
                    FileNode::file("README.md"),
                ])
                .expanded_paths(vec![PathBuf::from("src")])
                .selected_path(PathBuf::from("src/main.rs"))
                .show_file_size(false),
        )
        .child(label_chip("StatusBar", &theme))
        .child(self.nav_status_bar.clone())
        .child(label_chip("TopNav", &theme))
        .child(
            TopNav::new()
                .brand("Kael")
                .leading_icon("sparkles")
                .item(NavItem::new("Home").icon("home").selected(true))
                .item(NavItem::new("Projects").icon("folder"))
                .item(NavItem::new("Reports").icon("bar-chart-3"))
                .trailing(Button::new("nav-top-new", "New").size(ButtonSize::Sm)),
        )
        .child(label_chip("SideNav", &theme))
        .child(
            SideNav::new()
                .items(vec![
                    SideNavItem::new("overview".into(), "Overview").with_icon("layout-dashboard"),
                    SideNavItem::new("members".into(), "Members")
                        .with_icon("users")
                        .with_badge("4"),
                    SideNavItem::new("billing".into(), "Billing").with_icon("credit-card"),
                ])
                .selected_id("overview"),
        );

        // ===== Overlays =====
        let overlays_alert = section(
            "Alert dialog",
            "Confirmation prompt with cancel and action",
            &theme,
        )
        .child(
            div()
                .relative()
                .w_full()
                .h(px(260.0))
                .rounded(theme.tokens.radius_lg)
                .bg(theme.tokens.muted.opacity(0.25))
                .overflow_hidden()
                .child(self.overlays_alert_dialog.clone()),
        );

        let overlays_context_menu = section(
            "Context menu",
            "Right-click style menu with icons, shortcuts and a destructive action",
            &theme,
        )
        .child(
            div()
                .relative()
                .w_full()
                .h(px(280.0))
                .rounded(theme.tokens.radius_lg)
                .bg(theme.tokens.muted.opacity(0.25))
                .overflow_hidden()
                .child(
                    ContextMenu::new(point(px(24.0), px(20.0)))
                        .item(
                            ContextMenuItem::new("ctx-open", "Open")
                                .icon("file")
                                .shortcut("⌘O"),
                        )
                        .item(
                            ContextMenuItem::new("ctx-rename", "Rename")
                                .icon("pencil")
                                .shortcut("⌘R"),
                        )
                        .item(
                            ContextMenuItem::new("ctx-duplicate", "Duplicate")
                                .icon("copy")
                                .description("Create a copy in the same folder"),
                        )
                        .item(ContextMenuItem::separator())
                        .item(
                            ContextMenuItem::new("ctx-delete", "Delete")
                                .icon("trash")
                                .shortcut("⌫")
                                .destructive(true),
                        ),
                ),
        );

        let overlays_tooltip = section("Tooltip", "Pinned-open tooltips across placements", &theme)
            .child(
                row()
                    .gap(px(28.0))
                    .child(
                        col().child(label_chip("top", &theme)).child(
                            Tooltip::new("Tooltip on top")
                                .placement(TooltipPlacement::Top)
                                .default_open(true)
                                .child(
                                    Button::new("ovl-tip-top", "Hover")
                                        .variant(ButtonVariant::Outline),
                                ),
                        ),
                    )
                    .child(
                        col().child(label_chip("bottom", &theme)).child(
                            Tooltip::new("Tooltip on bottom")
                                .placement(TooltipPlacement::Bottom)
                                .default_open(true)
                                .child(
                                    Button::new("ovl-tip-bottom", "Hover")
                                        .variant(ButtonVariant::Outline),
                                ),
                        ),
                    )
                    .child(
                        col().child(label_chip("end aligned", &theme)).child(
                            Tooltip::new("Aligned to the end")
                                .placement(TooltipPlacement::Top)
                                .alignment(TooltipAlignment::End)
                                .default_open(true)
                                .child(
                                    Button::new("ovl-tip-end", "Hover")
                                        .variant(ButtonVariant::Outline),
                                ),
                        ),
                    )
                    .child(
                        col().child(label_chip("hover only", &theme)).child(
                            Tooltip::new("Appears on hover")
                                .placement(TooltipPlacement::Bottom)
                                .child(
                                    Button::new("ovl-tip-hover", "Hover me")
                                        .variant(ButtonVariant::Secondary),
                                ),
                        ),
                    ),
            );

        // ===== Typography =====
        let typography_heading = section(
            "Heading scale",
            "Semantic Heading levels and display variants",
            &theme,
        )
        .child(
            col()
                .child(label_chip("Levels", &theme))
                .child(Heading::new("Heading level 1").level(HeadingLevel::H1))
                .child(Heading::new("Heading level 3").level(HeadingLevel::H3))
                .child(Heading::new("Heading level 5").level(HeadingLevel::H5)),
        )
        .child(
            col()
                .child(label_chip("Display", &theme))
                .child(Heading::new("Display 1").heading_type(HeadingType::Display1))
                .child(Heading::new("Display 3").heading_type(HeadingType::Display3))
                .child(
                    Heading::new("Tinted heading")
                        .level(HeadingLevel::H3)
                        .color(theme.tokens.primary),
                ),
        );

        let typography_quote_gradient = section(
            "Blockquote & GradientText",
            "Quoted attribution and per-glyph gradient fills",
            &theme,
        )
        .child(
            col().child(label_chip("Blockquote", &theme)).child(
                Blockquote::new(body(
                    "Design is not just what it looks like. Design is how it works.".to_string(),
                ))
                .cite(caption("— Steve Jobs".to_string())),
            ),
        )
        .child(
            col()
                .child(label_chip("GradientText", &theme))
                .child(
                    GradientText::new("Astryx gradient")
                        .start_color(theme.tokens.primary)
                        .end_color(theme.tokens.accent_foreground)
                        .text_size(px(28.0))
                        .font_weight(FontWeight::BOLD),
                )
                .child(
                    GradientText::new("Multi-stop spectrum")
                        .colors(vec![
                            hsla(0.0, 0.8, 0.55, 1.0),
                            hsla(0.33, 0.8, 0.5, 1.0),
                            hsla(0.66, 0.8, 0.55, 1.0),
                        ])
                        .text_size(px(22.0))
                        .font_weight(FontWeight::SEMIBOLD),
                ),
        );

        let typography_links = section(
            "Link",
            "Inline, subtle, block and external link surfaces",
            &theme,
        )
        .child(
            row()
                .child(Link::new("Inline link").variant(LinkVariant::Inline))
                .child(
                    Link::new("Underlined")
                        .variant(LinkVariant::Inline)
                        .underline(true),
                )
                .child(Link::new("Subtle link").variant(LinkVariant::Subtle))
                .child(Link::new("External link").external(true))
                .child(
                    Link::new("Disabled link")
                        .variant(LinkVariant::Inline)
                        .disabled(true),
                ),
        )
        .child(
            col()
                .child(label_chip("Block", &theme))
                .child(Link::new("Block-style link row").variant(LinkVariant::Block)),
        );

        let typography_code = section(
            "Code & CodeBlock",
            "Inline code tokens and a syntax-highlighted block",
            &theme,
        )
        .child(
            row()
                .child(body("Run ".to_string()))
                .child(Code::new("cargo build").variant(CodeVariant::Inline))
                .child(body(" or ".to_string()))
                .child(Code::new("cargo test").variant(CodeVariant::Subtle)),
        )
        .child(
            CodeBlock::new(
                "fn main() {\n    let theme = Theme::astryx_neutral();\n    println!(\"{}\", theme.variant);\n}",
            )
            .language("rust")
            .show_line_numbers(true)
            .highlight_lines(vec![2])
            .max_height(px(160.0)),
        );

        let typography_kbd = section("KBD", "Keyboard shortcut chips at three sizes", &theme)
            .child(
                row()
                    .child(KBD::new("mod+k").size(KBDSize::Sm))
                    .child(KBD::new("mod+shift+p").size(KBDSize::Md))
                    .child(KBD::new("ctrl+alt+enter").size(KBDSize::Lg)),
            );

        let typography_motion = section(
            "Marquee & AnimatedText",
            "Looping ticker and staggered per-character entrance",
            &theme,
        )
        .child(
            col().child(label_chip("Marquee", &theme)).child(
                Marquee::new("typography-marquee", || {
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(24.0))
                        .child(body("Native Rust GPUI".to_string()))
                        .child(body("Astryx design system".to_string()))
                        .child(body("60fps typography".to_string()))
                        .into_any_element()
                })
                .speed(60.0)
                .direction(MarqueeDirection::Left)
                .pause_on_hover(true)
                .content_width(px(420.0))
                .w_full(),
            ),
        )
        .child(
            col()
                .child(label_chip("AnimatedText", &theme))
                .child(
                    AnimatedText::new("typography-anim-fade", "Fade up entrance")
                        .animation(TextAnimation::FadeUp)
                        .text_size(px(20.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.tokens.foreground),
                )
                .child(
                    AnimatedText::new("typography-anim-wave", "Wave motion")
                        .animation(TextAnimation::Wave)
                        .text_size(px(20.0))
                        .text_color(theme.tokens.primary),
                ),
        );

        // ===== Media =====
        let media_icons = section(
            "Icon Gallery",
            "IconSize scale and IconColor palette",
            &theme,
        )
        .child(
            col().child(label_chip("Sizes (IconSize)", &theme)).child(
                row()
                    .items_end()
                    .child(
                        Icon::new("search")
                            .size(IconSize::Xsm)
                            .icon_color(IconColor::Primary),
                    )
                    .child(
                        Icon::new("search")
                            .size(IconSize::Small)
                            .icon_color(IconColor::Primary),
                    )
                    .child(
                        Icon::new("search")
                            .size(IconSize::Sm)
                            .icon_color(IconColor::Primary),
                    )
                    .child(
                        Icon::new("search")
                            .size(IconSize::Md)
                            .icon_color(IconColor::Primary),
                    )
                    .child(
                        Icon::new("search")
                            .size(IconSize::Lg)
                            .icon_color(IconColor::Primary),
                    )
                    .child(
                        Icon::new("search")
                            .size(IconSize::Custom(px(40.0)))
                            .icon_color(IconColor::Primary),
                    ),
            ),
        )
        .child(
            col()
                .child(label_chip("Semantic colors (IconColor)", &theme))
                .child(
                    row()
                        .child(
                            Icon::new("success")
                                .size(IconSize::Md)
                                .icon_color(IconColor::Success),
                        )
                        .child(
                            Icon::new("warning")
                                .size(IconSize::Md)
                                .icon_color(IconColor::Warning),
                        )
                        .child(
                            Icon::new("error")
                                .size(IconSize::Md)
                                .icon_color(IconColor::Error),
                        )
                        .child(
                            Icon::new("info")
                                .size(IconSize::Md)
                                .icon_color(IconColor::Accent),
                        )
                        .child(
                            Icon::new("check")
                                .size(IconSize::Md)
                                .icon_color(IconColor::Primary),
                        )
                        .child(
                            Icon::new("clock")
                                .size(IconSize::Md)
                                .icon_color(IconColor::Secondary),
                        ),
                ),
        )
        .child(
            col()
                .child(label_chip("Hue palette (IconColor)", &theme))
                .child(
                    row()
                        .child(
                            Icon::new("calendar")
                                .size(IconSize::Md)
                                .icon_color(IconColor::Blue),
                        )
                        .child(
                            Icon::new("calendar")
                                .size(IconSize::Md)
                                .icon_color(IconColor::Cyan),
                        )
                        .child(
                            Icon::new("calendar")
                                .size(IconSize::Md)
                                .icon_color(IconColor::Teal),
                        )
                        .child(
                            Icon::new("calendar")
                                .size(IconSize::Md)
                                .icon_color(IconColor::Green),
                        )
                        .child(
                            Icon::new("calendar")
                                .size(IconSize::Md)
                                .icon_color(IconColor::Yellow),
                        )
                        .child(
                            Icon::new("calendar")
                                .size(IconSize::Md)
                                .icon_color(IconColor::Orange),
                        )
                        .child(
                            Icon::new("calendar")
                                .size(IconSize::Md)
                                .icon_color(IconColor::Red),
                        )
                        .child(
                            Icon::new("calendar")
                                .size(IconSize::Md)
                                .icon_color(IconColor::Pink),
                        )
                        .child(
                            Icon::new("calendar")
                                .size(IconSize::Md)
                                .icon_color(IconColor::Purple),
                        ),
                ),
        );

        let media_avatars = section("Avatars", "Avatar, status dots, and AvatarGroup", &theme)
            .child(
                col().child(label_chip("Sizes + fallbacks", &theme)).child(
                    row()
                        .child(Avatar::new().name("Ada Lovelace").size(AvatarSize::Xs))
                        .child(Avatar::new().name("Ada Lovelace").size(AvatarSize::Sm))
                        .child(Avatar::new().name("Ada Lovelace").size(AvatarSize::Md))
                        .child(
                            Avatar::new()
                                .name("Grace Hopper")
                                .size(AvatarSize::Lg)
                                .colorful(true),
                        )
                        .child(Avatar::new().fallback_text("?").size(AvatarSize::Lg))
                        .child(Avatar::new().size(AvatarSize::Lg)),
                ),
            )
            .child(
                col().child(label_chip("Status dots", &theme)).child(
                    row()
                        .child(
                            Avatar::new()
                                .name("Online User")
                                .size(AvatarSize::Lg)
                                .status_dot(
                                    AvatarStatusDot::new().variant(AvatarStatusDotVariant::Success),
                                ),
                        )
                        .child(
                            Avatar::new()
                                .name("Idle User")
                                .size(AvatarSize::Lg)
                                .status_dot(
                                    AvatarStatusDot::new().variant(AvatarStatusDotVariant::Neutral),
                                ),
                        )
                        .child(
                            Avatar::new()
                                .name("Busy User")
                                .size(AvatarSize::Lg)
                                .colorful(true)
                                .status_dot(
                                    AvatarStatusDot::new().variant(AvatarStatusDotVariant::Error),
                                ),
                        ),
                ),
            )
            .child(
                col()
                    .child(label_chip("AvatarGroup (max_visible + overflow)", &theme))
                    .child(
                        AvatarGroup::new(vec![
                            AvatarItem::new().name("Ada Lovelace"),
                            AvatarItem::new().name("Grace Hopper"),
                            AvatarItem::new().name("Alan Turing"),
                            AvatarItem::new().name("Katherine Johnson"),
                            AvatarItem::new().name("Edsger Dijkstra"),
                        ])
                        .size(AvatarSize::Md)
                        .max_visible(3)
                        .show_tooltips(true),
                    ),
            );

        let media_thumbnails = section("Thumbnail", "Square media preview states", &theme).child(
            row()
                .child(
                    col()
                        .child(label_chip("Placeholder", &theme))
                        .child(Thumbnail::new()),
                )
                .child(
                    col()
                        .child(label_chip("Loading", &theme))
                        .child(Thumbnail::new().loading(true)),
                )
                .child(
                    col()
                        .child(label_chip("Disabled", &theme))
                        .child(Thumbnail::new().disabled(true)),
                )
                .child(
                    col()
                        .child(label_chip("With label", &theme))
                        .child(Thumbnail::new().label("cover.png")),
                ),
        );

        let media_surfaces = section(
            "Surfaces & Effects",
            "GlassMorphism and GradientBorder",
            &theme,
        )
        .child(
            col()
                .child(label_chip("GlassMorphism (Light / Medium / Heavy)", &theme))
                .child(
                    row()
                        .child(
                            GlassMorphism::new()
                                .intensity(GlassIntensity::Light)
                                .w(px(120.0))
                                .h(px(72.0))
                                .p(px(12.0))
                                .child(body("Light").text_color(TextColor::Primary)),
                        )
                        .child(
                            GlassMorphism::new()
                                .intensity(GlassIntensity::Medium)
                                .w(px(120.0))
                                .h(px(72.0))
                                .p(px(12.0))
                                .child(body("Medium").text_color(TextColor::Primary)),
                        )
                        .child(
                            GlassMorphism::new()
                                .intensity(GlassIntensity::Heavy)
                                .tint(theme.tokens.primary)
                                .noise(true)
                                .w(px(120.0))
                                .h(px(72.0))
                                .p(px(12.0))
                                .child(body("Heavy + tint").text_color(TextColor::Primary)),
                        ),
                ),
        )
        .child(
            col().child(label_chip("GradientBorder", &theme)).child(
                row()
                    .child(
                        GradientBorder::new().width(px(2.0)).child(
                            div()
                                .w(px(140.0))
                                .h(px(64.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(body("Theme colors")),
                        ),
                    )
                    .child(
                        GradientBorder::new()
                            .colors(theme.tokens.success, theme.tokens.primary)
                            .width(px(3.0))
                            .rounded(px(16.0))
                            .child(
                                div()
                                    .w(px(140.0))
                                    .h(px(64.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(body("Custom colors")),
                            ),
                    ),
            ),
        );

        let media_layout = section("Layout & Backgrounds", "AspectRatio and DotPattern", &theme)
            .child(
                col()
                    .child(label_chip(
                        "AspectRatio (16:9 Rectangle / 1:1 Ellipse)",
                        &theme,
                    ))
                    .child(
                        row()
                            .items_start()
                            .child(
                                div().w(px(200.0)).child(AspectRatio::new(
                                    16.0 / 9.0,
                                    div()
                                        .size_full()
                                        .bg(theme.tokens.muted)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(caption("16 : 9")),
                                )),
                            )
                            .child(
                                div().w(px(96.0)).child(
                                    AspectRatio::new(
                                        1.0,
                                        div()
                                            .size_full()
                                            .bg(theme.tokens.accent)
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(caption("1 : 1")),
                                    )
                                    .shape(AspectRatioShape::Ellipse),
                                ),
                            ),
                    ),
            )
            .child(
                col().child(label_chip("DotPattern", &theme)).child(
                    div()
                        .w(px(320.0))
                        .h(px(120.0))
                        .rounded(theme.tokens.radius_md)
                        .overflow_hidden()
                        .border_1()
                        .border_color(theme.tokens.border)
                        .child(
                            DotPattern::new()
                                .spacing(px(18.0))
                                .dot_size(px(2.0))
                                .opacity(0.5)
                                .child(
                                    div()
                                        .size_full()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(body("Dotted backdrop")),
                                ),
                        ),
                ),
            );

        let media_gradient_text =
            section("Gradient Text", "Per-character color interpolation", &theme).child(
                col()
                    .child(
                        GradientText::new("Astryx Media")
                            .start_color(theme.tokens.primary)
                            .end_color(theme.tokens.success)
                            .text_size(px(28.0))
                            .font_weight(FontWeight::BOLD),
                    )
                    .child(
                        GradientText::new("Multi-stop gradient")
                            .colors(vec![
                                hsla(0.0, 0.8, 0.6, 1.0),
                                hsla(0.13, 0.85, 0.55, 1.0),
                                hsla(0.33, 0.7, 0.5, 1.0),
                                hsla(0.6, 0.8, 0.6, 1.0),
                            ])
                            .text_size(px(22.0))
                            .font_weight(FontWeight::SEMIBOLD),
                    ),
            );

        // ===== Layout =====
        let theme = theme.clone();

        let swatch = |bg: Hsla, label_text: &str| {
            div()
                .flex()
                .items_center()
                .justify_center()
                .h(px(56.0))
                .rounded(theme.tokens.radius_md)
                .bg(bg)
                .text_color(theme.tokens.foreground)
                .text_size(px(12.0))
                .child(label_text.to_string())
        };

        let layout_stack_section = section(
            "Stack, Center, Section & AspectRatio",
            "Flex containers, centering, surface regions and fixed ratios",
            &theme,
        )
        .child(
            col()
                .child(label_chip("Stack — vertical & horizontal", &theme))
                .child(
                    row()
                        .child(
                            Stack::new()
                                .vertical()
                                .gap(px(8.0))
                                .child(swatch(theme.tokens.muted, "1"))
                                .child(swatch(theme.tokens.muted, "2"))
                                .child(swatch(theme.tokens.muted, "3")),
                        )
                        .child(
                            Stack::new()
                                .horizontal()
                                .gap(px(8.0))
                                .align(Align::Center)
                                .justify(Justify::Between)
                                .w(px(280.0))
                                .child(StackItem::new(swatch(theme.tokens.muted, "fixed")))
                                .child(StackItem::new(swatch(theme.tokens.accent, "fill")).fill()),
                        ),
                ),
        )
        .child(
            col().child(label_chip("Center — both axes", &theme)).child(
                Center::new().axis(CenterAxis::Both).height(px(80.0)).child(
                    div()
                        .px(px(16.0))
                        .py(px(8.0))
                        .rounded(theme.tokens.radius_md)
                        .bg(theme.tokens.primary)
                        .text_color(theme.tokens.primary_foreground)
                        .child("Centered"),
                ),
            ),
        )
        .child(
            col()
                .child(label_chip("Section — variants & dividers", &theme))
                .child(
                    row()
                        .child(
                            Section::new()
                                .variant(SectionVariant::Section)
                                .child(body("Section surface")),
                        )
                        .child(
                            Section::new()
                                .variant(SectionVariant::Muted)
                                .divider(SectionDivider::Top)
                                .child(body("Muted + top divider")),
                        ),
                ),
        )
        .child(
            col()
                .child(label_chip("AspectRatio — 16:9 & ellipse", &theme))
                .child(
                    row()
                        .child(div().w(px(200.0)).child(AspectRatio::new(
                            16.0 / 9.0,
                            swatch(theme.tokens.accent, "16:9"),
                        )))
                        .child(
                            div().w(px(96.0)).child(
                                AspectRatio::new(1.0, swatch(theme.tokens.muted, "1:1"))
                                    .shape(AspectRatioShape::Ellipse),
                            ),
                        ),
                ),
        );

        let layout_grid_section = section(
            "Grid, GridSpan & MasonryGrid",
            "Column grids with spanning cells and masonry packing",
            &theme,
        )
        .child(
            col()
                .child(label_chip("Grid — 3 columns, GridSpan for header", &theme))
                .child(
                    Grid::new()
                        .columns(3)
                        .gap(px(8.0))
                        .alignment(GridAlignment::Stretch)
                        .w_full()
                        .child(
                            GridSpan::new()
                                .full()
                                .child(swatch(theme.tokens.primary, "header (span full)")),
                        )
                        .child(swatch(theme.tokens.muted, "a"))
                        .child(swatch(theme.tokens.muted, "b"))
                        .child(swatch(theme.tokens.muted, "c"))
                        .child(
                            GridSpan::new()
                                .columns(2)
                                .child(swatch(theme.tokens.accent, "span 2")),
                        )
                        .child(swatch(theme.tokens.muted, "d")),
                ),
        )
        .child(
            col()
                .child(label_chip("MasonryGrid — 3 columns", &theme))
                .child(
                    MasonryGrid::new()
                        .columns(3)
                        .gap(px(8.0))
                        .fill_width()
                        .item(swatch(theme.tokens.muted, "tall"), 120.0)
                        .item(swatch(theme.tokens.muted, "short"), 56.0)
                        .item(swatch(theme.tokens.muted, "mid"), 88.0)
                        .item(swatch(theme.tokens.muted, "short"), 56.0)
                        .item(swatch(theme.tokens.muted, "tall"), 120.0)
                        .item(swatch(theme.tokens.muted, "mid"), 88.0),
                ),
        );

        let layout_separator_section = section(
            "Separator / Divider",
            "Horizontal and vertical dividers with weights and labels",
            &theme,
        )
        .child(
            col()
                .w_full()
                .child(label_chip("Subtle & strong", &theme))
                .child(Separator::new())
                .child(Separator::new().variant(SeparatorVariant::Strong))
                .child(label_chip("With label", &theme))
                .child(Separator::new().label("OR"))
                .child(label_chip("Vertical", &theme))
                .child(
                    row()
                        .h(px(40.0))
                        .items_center()
                        .child(body("left"))
                        .child(Separator::new().orientation(SeparatorOrientation::Vertical))
                        .child(body("right")),
                ),
        );

        let collapsible_view = view.clone();
        let collapsible_open = self.layout_collapsible_open;
        let group_a = self.layout_collapsible_a;
        let group_b = self.layout_collapsible_b;
        let view_a = view.clone();
        let view_b = view.clone();

        let layout_collapsible_section = section(
            "Collapsible & CollapsibleGroup",
            "Expandable sections, optionally grouped with dividers",
            &theme,
        )
        .child(
            col()
                .w_full()
                .child(label_chip("Single collapsible", &theme))
                .child(
                    Collapsible::new()
                        .open(collapsible_open)
                        .trigger(body("Toggle details"))
                        .content(
                            div()
                                .p(px(8.0))
                                .child(muted("Hidden content revealed when open.")),
                        )
                        .on_toggle(move |open, _window, cx| {
                            collapsible_view.update(cx, |this, cx| {
                                this.layout_collapsible_open = open;
                                cx.notify();
                            });
                        }),
                ),
        )
        .child(
            col()
                .w_full()
                .child(label_chip("CollapsibleGroup — divided", &theme))
                .child(
                    CollapsibleGroup::new()
                        .divided(true)
                        .child(
                            Collapsible::new()
                                .open(group_a)
                                .trigger(body("First item"))
                                .content(div().p(px(8.0)).child(muted("First body.")))
                                .on_toggle(move |open, _window, cx| {
                                    view_a.update(cx, |this, cx| {
                                        this.layout_collapsible_a = open;
                                        cx.notify();
                                    });
                                }),
                        )
                        .child(
                            Collapsible::new()
                                .open(group_b)
                                .trigger(body("Second item"))
                                .content(div().p(px(8.0)).child(muted("Second body.")))
                                .on_toggle(move |open, _window, cx| {
                                    view_b.update(cx, |this, cx| {
                                        this.layout_collapsible_b = open;
                                        cx.notify();
                                    });
                                }),
                        ),
                ),
        );

        let carousel_slide = |text: &str, bg: Hsla| {
            CarouselSlide::new(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(px(140.0))
                    .w_full()
                    .bg(bg)
                    .text_color(theme.tokens.foreground)
                    .text_size(px(18.0))
                    .child(text.to_string()),
            )
        };

        let layout_panes_section = section(
            "SplitPane, Resizable & Carousel",
            "Draggable splits, resizable panel groups and slide carousels",
            &theme,
        )
        .child(
            col()
                .w_full()
                .child(label_chip("SplitPane — horizontal, collapsible", &theme))
                .child(
                    div().h(px(160.0)).w_full().child(
                        SplitPane::new(self.layout_split.clone())
                            .direction(SplitDirection::Horizontal)
                            .show_collapse_buttons(true)
                            .first(
                                div()
                                    .size_full()
                                    .bg(theme.tokens.muted)
                                    .p(px(12.0))
                                    .child(body("Pane A")),
                            )
                            .second(
                                div()
                                    .size_full()
                                    .bg(theme.tokens.card)
                                    .p(px(12.0))
                                    .child(body("Pane B")),
                            ),
                    ),
                ),
        )
        .child(
            col()
                .w_full()
                .child(label_chip("Resizable — horizontal group", &theme))
                .child(
                    div().h(px(140.0)).w_full().child(
                        h_resizable("layout-resizable", self.layout_resizable.clone())
                            .child(
                                resizable_panel().size(px(180.0)).child(
                                    div()
                                        .size_full()
                                        .bg(theme.tokens.muted)
                                        .p(px(12.0))
                                        .child(body("Sidebar")),
                                ),
                            )
                            .child(
                                resizable_panel().child(
                                    div()
                                        .size_full()
                                        .bg(theme.tokens.card)
                                        .p(px(12.0))
                                        .child(body("Main content")),
                                ),
                            ),
                    ),
                ),
        )
        .child(
            col()
                .w_full()
                .child(label_chip("Carousel — slide transition", &theme))
                .child(
                    Carousel::new("layout-carousel", self.layout_carousel.clone())
                        .size(CarouselSize::Md)
                        .transition(CarouselTransition::Slide)
                        .infinite(true)
                        .slide(carousel_slide("Slide 1", theme.tokens.muted))
                        .slide(carousel_slide("Slide 2", theme.tokens.accent))
                        .slide(carousel_slide("Slide 3", theme.tokens.card)),
                ),
        );

        let active = self.category;

        let sidebar = div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .w(px(232.0))
            .flex_shrink_0()
            .p(px(12.0))
            .border_r_1()
            .border_color(theme.tokens.border)
            .child(SideNavHeading::new("Components"))
            .children(ComponentCategory::ALL.into_iter().map(|cat| {
                let selected = cat == active;
                let view = view.clone();
                div()
                    .id(SharedString::from(format!("nav-{}", cat.id())))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .h(px(36.0))
                    .px(px(10.0))
                    .rounded(theme.tokens.radius_md)
                    .cursor_pointer()
                    .when(selected, |this| this.bg(theme.tokens.accent))
                    .child(
                        Icon::new(cat.icon())
                            .size(IconSize::Sm)
                            .icon_color(if selected {
                                IconColor::Primary
                            } else {
                                IconColor::Tertiary
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(if selected {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::MEDIUM
                            })
                            .text_color(if selected {
                                theme.tokens.foreground
                            } else {
                                theme.tokens.muted_foreground
                            })
                            .child(cat.label()),
                    )
                    .on_click(move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.category = cat;
                            cx.notify();
                        });
                    })
                    .into_any_element()
            }));

        let body = match active {
            ComponentCategory::Actions => col()
                .gap(px(20.0))
                .child(buttons)
                .child(actions_icon)
                .child(actions_grouped)
                .child(actions_fab),
            ComponentCategory::Inputs => col()
                .gap(px(20.0))
                .child(inputs)
                .child(more_inputs)
                .child(dropdowns)
                .child(otp_date)
                .child(pickers)
                .child(inputs_combobox_searchinput)
                .child(inputs_range_slider)
                .child(inputs_field_label)
                .child(inputs_field_status)
                .child(inputs_input_group)
                .child(inputs_tokenizer_typeahead),
            ComponentCategory::Selection => col()
                .gap(px(20.0))
                .child(selection)
                .child(controls)
                .child(rating_stepper)
                .child(selection_toggle_group)
                .child(selection_checkbox_list)
                .child(selection_multi_selector)
                .child(selection_animated_switch)
                .child(selection_dropdown),
            ComponentCategory::DataDisplay => col()
                .gap(px(20.0))
                .child(badges)
                .child(cards)
                .child(details)
                .child(data_table)
                .child(timeline_sec)
                .child(code_tags)
                .child(parity_surfaces)
                .child(dd_lists)
                .child(dd_tree)
                .child(dd_keys)
                .child(dd_misc),
            ComponentCategory::Charts => col()
                .gap(px(20.0))
                .child(charts_bar)
                .child(charts_line)
                .child(charts_area)
                .child(charts_pie_donut)
                .child(charts_gauge)
                .child(charts_sparkline)
                .child(charts_radar)
                .child(charts_heatmap),
            ComponentCategory::Feedback => col()
                .gap(px(20.0))
                .child(feedback)
                .child(extras)
                .child(misc)
                .child(empty_disclosure)
                .child(fb_circular)
                .child(fb_animated_progress)
                .child(fb_numbers)
                .child(fb_indicators)
                .child(fb_loading),
            ComponentCategory::Navigation => col()
                .gap(px(20.0))
                .child(nav_sec)
                .child(nav_disclosure)
                .child(nav_menus)
                .child(nav_toolbar)
                .child(nav_chrome),
            ComponentCategory::Overlays => col()
                .gap(px(20.0))
                .child(overlays)
                .child(modal_triggers)
                .child(overlays_alert)
                .child(overlays_context_menu)
                .child(overlays_tooltip),
            ComponentCategory::Typography => col()
                .gap(px(20.0))
                .child(typography)
                .child(typography_heading)
                .child(typography_quote_gradient)
                .child(typography_links)
                .child(typography_code)
                .child(typography_kbd)
                .child(typography_motion),
            ComponentCategory::Media => col()
                .gap(px(20.0))
                .child(media_icons)
                .child(media_avatars)
                .child(media_thumbnails)
                .child(media_surfaces)
                .child(media_layout)
                .child(media_gradient_text),
            ComponentCategory::Layout => col()
                .gap(px(20.0))
                .child(layout_stack_section)
                .child(layout_grid_section)
                .child(layout_separator_section)
                .child(layout_collapsible_section)
                .child(layout_panes_section),
        };

        let page = col()
            .gap(px(24.0))
            .p(px(32.0))
            .pb(px(64.0))
            .child(
                col()
                    .gap(px(4.0))
                    .child(h2(active.label().to_string()))
                    .child(muted(
                        "Live, interactive Astryx components — click and type to try them."
                            .to_string(),
                    )),
            )
            .child(body);

        let main = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.tokens.background)
            .text_color(theme.tokens.foreground)
            .font_family(theme.tokens.font_family.clone())
            .child(
                div()
                    .border_b_1()
                    .border_color(theme.tokens.border)
                    .child(header),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .child(sidebar)
                    .child(scrollable_vertical(page).size_full()),
            );

        div()
            .relative()
            .size_full()
            .child(main)
            .when(self.show_dialog, |this| this.child(self.dialog.clone()))
            .when(self.show_sheet, |this| this.child(self.sheet.clone()))
            .child(
                ToastViewport::new()
                    .manager(self.toasts.clone())
                    .position(ToastPosition::BottomEnd)
                    .max_visible(3),
            )
    }
}

struct Assets {
    base: std::path::PathBuf,
}

impl kael::AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        std::fs::read(self.base.join(path))
            .map(|data| Some(std::borrow::Cow::Owned(data)))
            .map_err(|err| err.into())
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        std::fs::read_dir(self.base.join(path))
            .map(|entries| {
                entries
                    .filter_map(|entry| {
                        entry
                            .ok()
                            .and_then(|entry| entry.file_name().into_string().ok())
                            .map(SharedString::from)
                    })
                    .collect()
            })
            .map_err(|err| err.into())
    }
}

fn main() {
    Application::new()
        .with_assets(Assets {
            base: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        })
        .run(move |cx| {
            kael_ui::init(cx);
            kael_ui::set_icon_base_path("assets/icons");
            install_theme(cx, Theme::astryx_neutral());

            let bounds = Bounds::centered(None, size(px(1400.0), px(1280.0)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Astryx · Kael UI".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |_, cx| cx.new(AstryxShowcase::new),
            )
            .unwrap();
        });
}

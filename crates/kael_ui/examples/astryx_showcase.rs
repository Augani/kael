use kael::{prelude::FluentBuilder as _, *};
use kael_ui::astryx::ControlSize;
use kael_ui::components::alert::Alert;
use kael_ui::components::audio_player::{AudioPlayer, AudioPlayerState};
use kael_ui::components::button_group::{ButtonGroup, ButtonGroupItem, ButtonGroupOrientation};
use kael_ui::components::code_block::CodeBlock;
use kael_ui::components::color_picker::{ColorPicker, ColorPickerState};
use kael_ui::components::date_picker::{DatePicker, DatePickerState, DateSelectionMode};
use kael_ui::components::field::FieldStatusType;
use kael_ui::components::file_upload::{FileUpload, FileUploadState};
use kael_ui::components::input::Input;
use kael_ui::components::input_state::InputState;
use kael_ui::components::navigation_menu::NavigationMenuOrientation;
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
use kael_ui::components::text::{Text, body, caption, code, h1, h2, h3, h4, h5, h6, label, muted};
use kael_ui::components::time_picker::{TimeFormat, TimePicker, TimePickerState};
use kael_ui::components::toggle_group::{ToggleGroup, ToggleGroupItem, ToggleGroupVariant};
use kael_ui::components::tooltip::{Tooltip, TooltipFocusTrigger, TooltipHoverIndication, tooltip};
use kael_ui::components::video_player::{VideoPlayer, VideoPlayerSize, VideoPlayerState};
use kael_ui::display::accordion::Accordion;
use kael_ui::display::data_table::{
    ColumnDef as DataTableColumnDef, DataTable as ProductionDataTable, RowAction,
};
use kael_ui::display::html::Html;
use kael_ui::display::markdown::Markdown;
use kael_ui::display::rich_text::{ListItem as RichListItem, RichBlock, RichInline, render_blocks};
use kael_ui::display::table::{
    Table, TableBody, TableCell, TableColumn, TableColumnAlign, TableDensity, TableDividers,
    TableFooter, TableHeader, TableHeaderCell, TableRow, TableTextOverflow, TableVerticalAlign,
    pixel, proportional,
};
use kael_ui::navigation::tabs::{TabVariant, Tabs, TabsLayout, TabsSize};
use kael_ui::navigation::{
    app_menu::{StandardMacMenuBar, file_menu, view_menu},
    breadcrumbs::{BreadcrumbItem, Breadcrumbs, BreadcrumbsVariant},
    nav_menu::NavMenu,
    virtual_list::v_virtual_list,
};
use kael_ui::overlays::bottom_sheet::{BottomSheet, BottomSheetSize};
use kael_ui::overlays::command_palette::{
    Command as UiCommand, CommandPalette as UiCommandPalette,
};
use kael_ui::overlays::dialog::{Dialog, DialogPurpose};
use kael_ui::overlays::hover_card::{HoverCard, HoverCardFocusTrigger, HoverCardHoverIndication};
use kael_ui::overlays::popover::{Popover, PopoverContent};
use kael_ui::overlays::popover_menu::{PopoverMenu, PopoverMenuItem};
use kael_ui::overlays::sheet::{Sheet, SheetSide, SheetSize};
use kael_ui::overlays::toast::{
    Toast, ToastItem, ToastManager, ToastPosition, ToastType, ToastVariant, ToastViewport,
};
use kael_ui::prelude::*;
use kael_ui::prelude::{
    AppShell, AppShellVariant, Avatar, AvatarGroup, AvatarGroupOverflow, AvatarItem, AvatarSize,
    AvatarStatusDot, AvatarStatusDotVariant, Badge, BadgeVariant, Banner, BannerContainer,
    BannerStatus, Button, ButtonSize, ButtonVariant, Calendar, Card, Chat, ChatMessage,
    ChatMessageRole, Checkbox, CheckboxList, CheckboxListItem, Citation, CitationVariant,
    ClickableCard, Code, CodeVariant, Collapsible, CollapsibleGroup, CommandPaletteEmpty,
    CommandPaletteFooter, CommandPaletteGroup, CommandPaletteInput, CommandPaletteItem,
    CommandPaletteList, ContextMenu, ContextMenuItem, DateValue, DayOfWeek, Divider,
    DividerVariant, DropdownMenuItemData, EmptyState, Grid, GridAlignment, GridSpan, Heading,
    HeadingLevel, HeadingType, Hue, Icon, IconButton, IconColor, IconRegistry, IconSize,
    InputGroup, InputGroupText, InputSize, InteractiveRole, InteractiveRoleContext, Item, KBD,
    Layer, LayerAlignment, LayerPlacement, LayerProvider, LayerToastConfig, Layout, LayoutContent,
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
    TypeaheadItem, create_static_source, register_icons,
};
use kael_ui::theme::{Theme, ThemeTokens, ThemeVariant, install_theme, use_theme};
use std::{path::PathBuf, rc::Rc};

use kael::Axis;
use kael_ui::components::label::Label;
use kael_ui::components::progress::SpinnerType;
use kael_ui::components::resizable::{h_resizable, resizable_panel};
use kael_ui::components::split_pane::SplitDirection;
use kael_ui::navigation::menu::{Menu, MenuItem};
use kael_ui::navigation::status_bar::{StatusBar, StatusItem};

actions!(
    astryx_showcase_menu,
    [ShowNavigation, ShowOverlays, QuitShowcase]
);

#[derive(Clone)]
struct ShowcaseRecord {
    project: String,
    owner: String,
    budget: f64,
    active: bool,
}

struct AstryxShowcase {
    terms: bool,
    notifications: bool,
    marketing: bool,
    plan: usize,
    card_pick: usize,
    page: usize,
    table_sort_column: usize,
    table_sort_direction: TableSortDirection,
    data_grid: Entity<DataGridState<ShowcaseRecord>>,
    production_data_table: Entity<ProductionDataTable<ShowcaseRecord>>,
    dd_power_query: Entity<InputState>,
    dd_power_filters: Vec<PowerSearchFilter>,
    dd_show_removable_token: bool,
    acc_open: std::collections::HashSet<usize>,
    segmented: Entity<SegmentedNavState>,
    slider: Entity<SliderState>,
    select: Entity<Select<String>>,
    number: Entity<NumberInputState>,
    rating: Entity<RatingState>,
    stepper: Entity<StepperState>,
    otp: Entity<OTPState>,
    chat_messages: Vec<ChatMessage>,
    chat_composer: Entity<InputState>,
    tags: Entity<TagInputState>,
    date: Entity<DatePickerState>,
    inputs_calendar_month: DateValue,
    inputs_calendar_date: DateValue,
    file_state: Entity<FileUploadState>,
    color_state: Entity<ColorPickerState>,
    time_state: Entity<TimePickerState>,
    time_12_state: Entity<TimePickerState>,
    time_seconds_state: Entity<TimePickerState>,
    time_read_only_state: Entity<TimePickerState>,
    inputs_date_range: Entity<DatePickerState>,
    inputs_datetime_date: Entity<DatePickerState>,
    inputs_datetime_time: Entity<TimePickerState>,
    inputs_hotkey: Entity<HotkeyInputState>,
    inputs_inline_edit: Entity<InlineEditState>,
    inputs_mention: Entity<MentionInputState>,
    inputs_text_field: Entity<TextFieldState>,
    command_input: Entity<InputState>,
    field_search: Entity<InputState>,
    field_email: Entity<InputState>,
    field_invalid: Entity<InputState>,
    field_disabled: Entity<InputState>,
    field_textarea: SharedString,
    actions_copy: Entity<CopyButtonState>,
    actions_fab: Entity<FABState>,
    inputs_combobox: Entity<Combobox<String>>,
    inputs_search: Entity<SearchInput>,
    inputs_range: Entity<RangeSliderState>,
    inputs_range_large: Entity<RangeSliderState>,
    inputs_range_disabled: Entity<RangeSliderState>,
    inputs_typeahead_value: Option<TypeaheadItem>,
    inputs_typeahead_query: SharedString,
    selection_toggle_value: SharedString,
    selection_toggle_views: Vec<SharedString>,
    selection_checks: Vec<SharedString>,
    selection_switch_active: usize,
    selection_assignees: Vec<SharedString>,
    selection_dropdown: Entity<DropdownState>,
    dd_expandable: Entity<ExpandableCardState>,
    dd_shortcuts: Entity<KeyboardShortcuts>,
    dd_outline_selected: SharedString,
    dd_source_link_status: SharedString,
    fb_countdown: Entity<CountdownState>,
    fb_counter: Entity<AnimatedCounterState>,
    fb_notifications: Entity<NotificationCenterState>,
    fb_skeleton: Entity<SkeletonLoaderState>,
    show_fb_notifications: bool,
    typography_typewriter: Entity<TypeWriterState>,
    media_audio: Entity<AudioPlayerState>,
    media_video: Entity<VideoPlayerState>,
    media_viewer: Entity<ImageViewer>,
    show_media_viewer: bool,
    nav_menu_bar: Entity<MenuBar>,
    nav_status_bar: Entity<StatusBar>,
    nav_selected_path: PathBuf,
    nav_expanded_paths: Vec<PathBuf>,
    nav_tab_primary: usize,
    nav_tab_period: usize,
    nav_tab_mail: SharedString,
    nav_toolbar_bold: bool,
    nav_toolbar_italic: bool,
    nav_row_selected: SharedString,
    nav_top_selected: SharedString,
    nav_side_selected: SharedString,
    nav_side_collapsed: bool,
    nav_mobile_open: bool,
    nav_breadcrumb_selected: SharedString,
    nav_hierarchy_selected: SharedString,
    nav_hierarchy_expanded: Vec<SharedString>,
    overlays_alert_dialog: Entity<AlertDialog>,
    overlays_command_palette: Entity<UiCommandPalette>,
    show_alert_dialog: bool,
    show_command_palette: bool,
    show_bottom_sheet: bool,
    show_popover_menu: bool,
    popover_menu_position: Point<Pixels>,
    layout_collapsible_open: bool,
    layout_collapsible_a: bool,
    layout_collapsible_b: bool,
    layout_split: Entity<SplitPaneState>,
    layout_resizable: Entity<ResizableState>,
    layout_carousel: Entity<CarouselState>,
    layout_animated_list: Entity<AnimatedListState>,
    layout_animated_item_count: usize,
    layout_presence: Entity<AnimatedPresenceState>,
    layout_presence_visible: bool,
    layout_transition_version: usize,
    layout_shared: Entity<SharedElementState>,
    layout_shared_at_target: bool,
    layout_sortable: Entity<SortableListState<SharedString>>,
    layout_infinite: Entity<InfiniteScrollState>,
    layout_infinite_count: usize,
    layout_drawer: Entity<DrawerState>,
    layout_drag_drop: Entity<DragDropKeyboardState<SharedString>>,
    layout_last_drop: SharedString,
    effects_confetti: Entity<ConfettiState>,
    effects_aurora: Entity<AuroraState>,
    effects_meteors: Entity<MeteorState>,
    effects_particles: Entity<ParticleEmitterState>,
    effects_spotlight: Entity<SpotlightState>,
    effects_tilt: Entity<TiltCardState>,
    effects_dock: Entity<DockState>,
    effects_drag: Entity<DraggableSpringState>,
    effects_magnetic: Entity<MagneticButtonState>,
    effects_crop: Entity<CropAreaState>,
    effects_ripple_version: usize,
    page_scroll: ScrollHandle,
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

    fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|category| category.id().eq_ignore_ascii_case(id.trim()))
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
                .title("Project settings")
                .description("Keep focused tasks in context.")
                .purpose(DialogPurpose::Info)
                .content_builder(|| {
                    col()
                        .gap(px(14.0))
                        .child(Alert::info().title("Desktop-ready defaults").description(
                            "Focus return, Escape dismissal and backdrop handling are enabled.",
                        ))
                        .child(
                            MetadataList::new()
                                .columns(MetadataListColumns::Count(2))
                                .item(MetadataListItem::new("Density", "Comfortable"))
                                .item(MetadataListItem::new("Theme", "System"))
                                .item(MetadataListItem::new("Autosave", "Enabled"))
                                .item(MetadataListItem::new("Status", "Ready")),
                        )
                })
                .footer_builder(|| {
                    row()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .child(caption("Changes save automatically".to_string()))
                        .child(Badge::new("Up to date").variant(BadgeVariant::Success))
                })
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
                .size(SheetSize::Sm)
                .title("Component details")
                .description("Inspect metadata in context.")
                .content_builder(|| {
                    col()
                        .gap(px(20.0))
                        .p(px(24.0))
                        .child(
                            col()
                                .gap(px(6.0))
                                .child(label("Status".to_string()))
                                .child(
                                    row()
                                        .gap(px(8.0))
                                        .child(
                                            Badge::new("Accessible")
                                                .variant(BadgeVariant::Success),
                                        )
                                        .child(Badge::new("Stable").variant(BadgeVariant::Info)),
                                ),
                        )
                        .child(
                            col()
                                .gap(px(6.0))
                                .child(label("Purpose".to_string()))
                                .child(body(
                                    "Sheets keep secondary context close while the main workspace stays visible."
                                        .to_string(),
                                )),
                        )
                        .child(
                            col()
                                .gap(px(6.0))
                                .child(label("Keyboard".to_string()))
                                .child(row().gap(px(8.0)).child(KBD::new("esc")).child(muted(
                                    "Close and return to the previous task.".to_string(),
                                ))),
                        )
                })
                .footer_builder(|| {
                    row()
                        .items_center()
                        .justify_between()
                        .child(caption("Updated just now".to_string()))
                        .child(Badge::new("Ready").variant(BadgeVariant::Success))
                })
                .on_close(move |_window, cx| {
                    view.update(cx, |this, cx| {
                        this.show_sheet = false;
                        cx.notify();
                    });
                })
        });
        let alert_dialog = cx.new(|cx| {
            let cancel_view = view.clone();
            let action_view = view.clone();
            AlertDialog::new(cx)
                .title("Delete project?")
                .description(
                    "This permanently removes the project and all of its files. This action cannot be undone.",
                )
                .cancel_text("Cancel")
                .action_text("Delete")
                .destructive(true)
                .on_cancel(move |_, cx| {
                    cancel_view.update(cx, |this, cx| {
                        this.show_alert_dialog = false;
                        cx.notify();
                    });
                })
                .on_action(move |_, cx| {
                    action_view.update(cx, |this, cx| {
                        this.show_alert_dialog = false;
                        cx.notify();
                    });
                })
        });
        let command_palette = cx.new(|cx| {
            let close_view = view.clone();
            let create_view = view.clone();
            let review_view = view.clone();
            let theme_view = view.clone();
            UiCommandPalette::from_commands(
                cx,
                vec![
                    UiCommand::new("create-component", "Create component")
                        .description("Add an accessible component to the current workspace")
                        .icon("plus")
                        .category("Workspace")
                        .shortcut("⌘N")
                        .on_select(move |window, cx| {
                            create_view.update(cx, |this, cx| {
                                this.toast_n += 1;
                                let id = this.toast_n;
                                this.toasts.update(cx, |manager, cx| {
                                    manager.add_toast(
                                        ToastItem::new(id, "Component draft created")
                                            .variant(ToastVariant::Success),
                                        window,
                                        cx,
                                    );
                                });
                            });
                        }),
                    UiCommand::new("review-accessibility", "Review accessibility")
                        .description("Open the keyboard and screen-reader checklist")
                        .icon("accessibility")
                        .category("Quality")
                        .shortcut("⌘⇧A")
                        .on_select(move |window, cx| {
                            review_view.update(cx, |this, cx| {
                                this.toast_n += 1;
                                let id = this.toast_n;
                                this.toasts.update(cx, |manager, cx| {
                                    manager.add_toast(
                                        ToastItem::new(id, "Accessibility review ready")
                                            .variant(ToastVariant::Default),
                                        window,
                                        cx,
                                    );
                                });
                            });
                        }),
                    UiCommand::new("switch-theme", "Switch color theme")
                        .description("Cycle through the Astryx showcase palettes")
                        .icon("palette")
                        .category("Appearance")
                        .shortcut("⌘⇧T")
                        .on_select(move |window, cx| {
                            theme_view.update(cx, |this, cx| {
                                this.toast_n += 1;
                                let id = this.toast_n;
                                this.toasts.update(cx, |manager, cx| {
                                    manager.add_toast(
                                        ToastItem::new(id, "Theme command selected")
                                            .variant(ToastVariant::Default),
                                        window,
                                        cx,
                                    );
                                });
                            });
                        }),
                ],
            )
            .id("astryx-command-palette")
            .on_close(move |_, cx| {
                close_view.update(cx, |this, cx| {
                    this.show_command_palette = false;
                    cx.notify();
                });
            })
        });
        let toasts = cx.new(|cx| ToastManager::new(cx).position(ToastPosition::BottomRight));
        let inputs_combobox_state = cx.new(|_| ComboboxState::new());
        let typography_typewriter = cx.new(|_| {
            TypeWriterState::new("Accessible components should feel effortless to use.")
                .with_speed(std::time::Duration::from_millis(38))
        });
        let typewriter = typography_typewriter.clone();
        cx.spawn(async move |_this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(350))
                .await;
            _ = typewriter.update(cx, |state, cx| state.start(cx));
        })
        .detach();
        let media_audio = cx.new(|cx| {
            let mut state = AudioPlayerState::new(cx);
            state.set_duration(214.0, cx);
            state.set_current_time(68.0, cx);
            state
        });
        let media_video = cx.new(|cx| {
            let mut state = VideoPlayerState::new(cx);
            state.set_title("Astryx component tour", cx);
            state.set_frame("assets/images/carousel_1.jpg", cx);
            state.set_duration(148.0, cx);
            state.set_current_time(42.0, cx);
            state
        });
        let media_viewer_state = cx.new(|_| {
            ImageViewerState::new(vec![
                ImageItem::new("assets/images/carousel_1.jpg")
                    .alt("A pale antler draped with crimson fabric on a star-speckled backdrop")
                    .caption("Crimson and antler"),
                ImageItem::new("assets/images/carousel_2.jpg")
                    .alt("Golden sunlight breaking through mist over forested hills")
                    .caption("Golden mountain mist"),
                ImageItem::new("assets/images/carousel_3.jpg")
                    .alt("A translucent spider web stretched across moss on a forest tree")
                    .caption("Web on moss"),
            ])
        });
        let media_viewer = cx.new(|cx| {
            let view = view.clone();
            ImageViewer::new(media_viewer_state, cx)
                .show_thumbnails(true)
                .has_zoom(true)
                .on_close(move |_, cx| {
                    view.update(cx, |this, cx| {
                        this.show_media_viewer = false;
                        cx.notify();
                    });
                })
        });
        let category = std::env::var("ASTRYX_SHOWCASE_CATEGORY")
            .ok()
            .and_then(|category| ComponentCategory::from_id(&category))
            .unwrap_or(ComponentCategory::Actions);
        let overlay_open = std::env::var("ASTRYX_SHOWCASE_OVERLAY_OPEN").ok();
        let data_grid = cx.new(|_| {
            DataGridState::new(
                vec![
                    ShowcaseRecord {
                        project: "Desktop shell".into(),
                        owner: "Platform".into(),
                        budget: 84_000.0,
                        active: true,
                    },
                    ShowcaseRecord {
                        project: "Component audit".into(),
                        owner: "Design systems".into(),
                        budget: 52_500.0,
                        active: true,
                    },
                    ShowcaseRecord {
                        project: "Legacy migration".into(),
                        owner: "Infrastructure".into(),
                        budget: 31_200.0,
                        active: false,
                    },
                ],
                vec![
                    GridColumnDef::new(
                        "project",
                        "Project",
                        |record: &ShowcaseRecord, _| {
                            div().child(record.project.clone()).into_any_element()
                        },
                        |record: &ShowcaseRecord| record.project.clone(),
                    )
                    .width(px(230.0))
                    .min_width(px(160.0))
                    .sortable(true)
                    .editable(true)
                    .value_setter(|record, value| record.project = value.to_string()),
                    GridColumnDef::new(
                        "owner",
                        "Owner",
                        |record: &ShowcaseRecord, _| {
                            div().child(record.owner.clone()).into_any_element()
                        },
                        |record: &ShowcaseRecord| record.owner.clone(),
                    )
                    .width(px(180.0))
                    .min_width(px(130.0))
                    .sortable(true),
                    GridColumnDef::new(
                        "budget",
                        "Budget",
                        |record: &ShowcaseRecord, _| {
                            div()
                                .child(format!("${:.0}", record.budget))
                                .into_any_element()
                        },
                        |record: &ShowcaseRecord| record.budget.to_string(),
                    )
                    .width(px(140.0))
                    .min_width(px(110.0))
                    .sortable(true)
                    .editable(true)
                    .editor(CellEditor::Number)
                    .value_setter(|record, value| {
                        if let Ok(value) = value.parse::<f64>()
                            && value.is_finite()
                        {
                            record.budget = value;
                        }
                    }),
                    GridColumnDef::new(
                        "active",
                        "Active",
                        |record: &ShowcaseRecord, _| {
                            Badge::new(if record.active { "Active" } else { "Paused" })
                                .variant(if record.active {
                                    BadgeVariant::Success
                                } else {
                                    BadgeVariant::Neutral
                                })
                                .into_any_element()
                        },
                        |record: &ShowcaseRecord| record.active.to_string(),
                    )
                    .width(px(120.0))
                    .resizable(false)
                    .sortable(true)
                    .editable(true)
                    .editor(CellEditor::Checkbox)
                    .value_setter(|record, value| {
                        record.active = value.parse::<bool>().unwrap_or(record.active);
                    }),
                ],
            )
        });
        let production_data_table = cx.new(|cx| {
            ProductionDataTable::new(
                vec![
                    ShowcaseRecord {
                        project: "Desktop shell".into(),
                        owner: "Platform".into(),
                        budget: 84_000.0,
                        active: true,
                    },
                    ShowcaseRecord {
                        project: "Component audit".into(),
                        owner: "Design systems".into(),
                        budget: 52_500.0,
                        active: true,
                    },
                    ShowcaseRecord {
                        project: "Legacy migration".into(),
                        owner: "Infrastructure".into(),
                        budget: 31_200.0,
                        active: false,
                    },
                    ShowcaseRecord {
                        project: "Accessibility pass".into(),
                        owner: "Quality".into(),
                        budget: 46_800.0,
                        active: true,
                    },
                ],
                vec![
                    DataTableColumnDef::new("project", "Project", |record: &ShowcaseRecord| {
                        record.project.clone().into()
                    })
                    .width(px(230.0))
                    .min_width(px(160.0)),
                    DataTableColumnDef::new("owner", "Owner", |record: &ShowcaseRecord| {
                        record.owner.clone().into()
                    })
                    .width(px(180.0))
                    .min_width(px(130.0)),
                    DataTableColumnDef::new("budget", "Budget", |record: &ShowcaseRecord| {
                        format!("${:.0}", record.budget).into()
                    })
                    .width(px(140.0))
                    .min_width(px(110.0)),
                    DataTableColumnDef::new("active", "Status", |record: &ShowcaseRecord| {
                        if record.active { "Active" } else { "Paused" }.into()
                    })
                    .width(px(120.0))
                    .resizable(false),
                ],
                cx,
            )
            .id("showcase-production-data-table")
            .show_selection(true)
            .row_actions(vec![
                RowAction::new("open", "Open", |_, _, _| {}).icon("external-link"),
                RowAction::new("archive", "Archive", |_, _, _| {}).icon("archive"),
            ])
        });

        let layout_animated_list = cx.new(AnimatedListState::new);
        layout_animated_list.update(cx, |state, cx| {
            state.set_keys(vec!["design".into(), "build".into(), "review".into()], cx);
        });
        let layout_presence = cx.new(|_| AnimatedPresenceState::new());
        layout_presence.update(cx, |state, cx| state.set_visible(true, cx));
        let page_scroll = ScrollHandle::new();
        if std::env::var("ASTRYX_SHOWCASE_START_AT_BOTTOM").as_deref() == Ok("1") {
            let audit_scroll = page_scroll.clone();
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(250))
                    .await;
                audit_scroll.scroll_to_bottom();
                _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }

        Self {
            terms: true,
            notifications: true,
            marketing: false,
            plan: 1,
            card_pick: 1,
            page: 3,
            table_sort_column: 0,
            table_sort_direction: TableSortDirection::Ascending,
            data_grid,
            production_data_table,
            dd_power_query: cx.new(|cx| {
                let mut state = InputState::new(cx);
                state.content = "release".into();
                state
            }),
            dd_power_filters: vec![
                PowerSearchFilter::new("status", "is", "active"),
                PowerSearchFilter::new("owner", "contains", "design"),
            ],
            dd_show_removable_token: true,
            acc_open: std::collections::HashSet::from([0]),
            segmented: cx.new(|_cx| SegmentedNavState::new("grid")),
            slider,
            select,
            number: cx.new(NumberInputState::new),
            rating: cx.new(RatingState::new),
            stepper,
            otp: cx.new(|cx| OTPState::new(cx, 6)),
            chat_messages: vec![
                ChatMessage::new("system", "Astryx parity audit started.")
                    .role(ChatMessageRole::System)
                    .timestamp("09:41"),
                ChatMessage::new(
                    "assistant",
                    "Table, TreeList and tokenized search surfaces now render in the showcase.",
                )
                .author("Kael")
                .timestamp("09:42"),
                ChatMessage::new("user", "Run the visual QA pass next.")
                    .role(ChatMessageRole::User)
                    .author("You")
                    .timestamp("09:43"),
            ],
            chat_composer: cx.new(|cx| InputState::new(cx).placeholder("Message…")),
            tags: cx.new(TagInputState::new),
            date: cx.new(|cx| DatePickerState::new(cx)),
            inputs_calendar_month: DateValue::new(2026, 6, 1),
            inputs_calendar_date: DateValue::new(2026, 6, 26),
            file_state: cx.new(|_| FileUploadState::new()),
            color_state: cx.new(|_| ColorPickerState::new(kael::hsla(0.62, 0.7, 0.5, 1.0))),
            time_state: cx.new(TimePickerState::new),
            time_12_state: cx.new(|cx| {
                let mut state = TimePickerState::new(cx);
                state.set_format(TimeFormat::Hour12, cx);
                state
            }),
            time_seconds_state: cx.new(|cx| {
                let mut state = TimePickerState::new(cx);
                state.set_show_seconds(true, cx);
                state
            }),
            time_read_only_state: cx.new(TimePickerState::new),
            inputs_date_range: cx.new(|cx| {
                let mut state = DatePickerState::new_with_mode(DateSelectionMode::Range, cx);
                state.selected_range = Some(DateRange::new(
                    DateValue::new(2026, 7, 14),
                    DateValue::new(2026, 7, 18),
                ));
                state.viewing_month = DateValue::new(2026, 7, 1);
                state
            }),
            inputs_datetime_date: cx
                .new(|cx| DatePickerState::new_with_date(DateValue::new(2026, 7, 22), cx)),
            inputs_datetime_time: cx.new(|cx| {
                let mut state = TimePickerState::new(cx);
                state.set_value(TimeValue::new(14, 30), cx);
                state
            }),
            inputs_hotkey: cx.new(|cx| {
                HotkeyInputState::with_hotkey(
                    cx,
                    HotkeyValue::new(
                        "k",
                        Modifiers {
                            platform: true,
                            shift: true,
                            ..Modifiers::default()
                        },
                    ),
                )
            }),
            inputs_inline_edit: cx
                .new(|cx| InlineEditState::with_value(cx, "Accessible desktop components")),
            inputs_mention: cx.new(MentionInputState::new),
            inputs_text_field: cx.new(|cx| {
                let mut state = TextFieldState::new(cx);
                state.set_text("Astryx workspace".into(), cx);
                state
            }),
            command_input: cx.new(|cx| InputState::new(cx).placeholder("Search commands...")),
            field_search: cx.new(|cx| InputState::new(cx).placeholder("Search…")),
            field_email: cx.new(|cx| InputState::new(cx).placeholder("you@example.com")),
            field_invalid: cx.new(|cx| InputState::new(cx).placeholder("Required field")),
            field_disabled: cx.new(|cx| InputState::new(cx).placeholder("Disabled")),
            field_textarea: SharedString::default(),
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
            inputs_range_large: cx.new(RangeSliderState::new),
            inputs_range_disabled: cx.new(RangeSliderState::new),
            inputs_typeahead_value: Some(TypeaheadItem::new("Grace Hopper", "grace")),
            inputs_typeahead_query: SharedString::default(),
            selection_toggle_value: "bold".into(),
            selection_toggle_views: vec!["grid".into()],
            selection_checks: vec!["analytics".into(), "updates".into()],
            selection_switch_active: 0,
            selection_assignees: vec!["ada".into(), "alan".into(), "grace".into()],
            selection_dropdown: cx.new(DropdownState::new),
            dd_expandable: cx.new(|_| ExpandableCardState::new()),
            dd_shortcuts: cx.new(|_| {
                KeyboardShortcuts::new()
                    .category(
                        "Editing",
                        vec![
                            ShortcutItem::new("Copy", "cmd-c"),
                            ShortcutItem::new("Paste", "cmd-v"),
                            ShortcutItem::new("Undo", "cmd-z"),
                        ],
                    )
                    .category(
                        "Navigation",
                        vec![
                            ShortcutItem::new("Command palette", "cmd-shift-p"),
                            ShortcutItem::new("Go to file", "cmd-p"),
                        ],
                    )
            }),
            dd_outline_selected: "inputs".into(),
            dd_source_link_status: "No source link activated yet".into(),
            fb_countdown: cx.new(|cx| {
                let mut s = CountdownState::new(cx);
                s.set_duration(
                    std::time::Duration::from_secs(2 * 86400 + 5 * 3600 + 30 * 60 + 15),
                    cx,
                );
                s
            }),
            fb_counter: cx.new(|_| AnimatedCounterState::new(1280.0)),
            fb_notifications: cx.new(|cx| {
                let mut s = NotificationCenterState::new(cx);
                s.add(
                    NotificationItem::new("fb-n3", "Deployment succeeded")
                        .message("v2.4.0 is live in production.")
                        .variant(NotificationVariant::Success),
                    cx,
                );
                s.add(
                    NotificationItem::new("fb-n2", "Storage almost full")
                        .message("You have used 92% of your quota.")
                        .variant(NotificationVariant::Warning),
                    cx,
                );
                s.add(
                    NotificationItem::new("fb-n1", "New comment on your PR")
                        .variant(NotificationVariant::Info),
                    cx,
                );
                s
            }),
            fb_skeleton: cx.new(|_| SkeletonLoaderState::new()),
            show_fb_notifications: false,
            typography_typewriter,
            media_audio,
            media_video,
            media_viewer,
            show_media_viewer: false,
            nav_menu_bar: cx.new(|_cx| {
                MenuBar::new(vec![
                    MenuBarItem::new("file", "File").with_items(vec![
                        MenuItem::new("new", "New File")
                            .with_icon("file-plus")
                            .with_shortcut("\u{2318}N"),
                        MenuItem::new("open", "Open")
                            .with_icon("folder-open")
                            .with_shortcut("\u{2318}O"),
                        MenuItem::separator(),
                        MenuItem::new("quit", "Quit").with_shortcut("\u{2318}Q"),
                    ]),
                    MenuBarItem::new("edit", "Edit").with_items(vec![
                        MenuItem::new("undo", "Undo").with_shortcut("\u{2318}Z"),
                        MenuItem::new("redo", "Redo").disabled(true),
                    ]),
                    MenuBarItem::new("view", "View").with_items(vec![
                        MenuItem::new("zoom-in", "Zoom In").with_shortcut("⌘+"),
                        MenuItem::new("zoom-out", "Zoom Out").with_shortcut("⌘−"),
                        MenuItem::separator(),
                        MenuItem::checkbox("sidebar", "Show Sidebar", true),
                    ]),
                ])
            }),
            nav_status_bar: cx.new(|_cx| {
                StatusBar::new()
                    .left(vec![
                        StatusItem::icon_text("git-branch", "main"),
                        StatusItem::icon_text("circle-dot", "3 issues"),
                    ])
                    .center(vec![StatusItem::text("UTF-8")])
                    .right(vec![
                        StatusItem::badge("2", "2 warnings").badge_variant(BadgeVariant::Warning),
                        StatusItem::icon_text("check-circle", "Ready"),
                    ])
            }),
            nav_selected_path: PathBuf::from("src/main.rs"),
            nav_expanded_paths: vec![PathBuf::from("src")],
            nav_tab_primary: 0,
            nav_tab_period: 1,
            nav_tab_mail: "inbox".into(),
            nav_toolbar_bold: true,
            nav_toolbar_italic: false,
            nav_row_selected: "dashboard".into(),
            nav_top_selected: "home".into(),
            nav_side_selected: "overview".into(),
            nav_side_collapsed: false,
            nav_mobile_open: std::env::var("ASTRYX_SHOWCASE_MOBILE_OPEN").as_deref() == Ok("1"),
            nav_breadcrumb_selected: "component".into(),
            nav_hierarchy_selected: "overview".into(),
            nav_hierarchy_expanded: vec!["workspace".into()],
            overlays_alert_dialog: alert_dialog,
            overlays_command_palette: command_palette,
            show_alert_dialog: overlay_open.as_deref() == Some("alert"),
            show_command_palette: overlay_open.as_deref() == Some("command-palette"),
            show_bottom_sheet: overlay_open.as_deref() == Some("bottom-sheet"),
            show_popover_menu: overlay_open.as_deref() == Some("popover-menu"),
            popover_menu_position: point(px(720.0), px(300.0)),
            layout_collapsible_open: true,
            layout_collapsible_a: true,
            layout_collapsible_b: false,
            layout_split: cx.new(SplitPaneState::new),
            layout_resizable: ResizableState::new(cx),
            layout_carousel: cx.new(CarouselState::new),
            layout_animated_list,
            layout_animated_item_count: 3,
            layout_presence,
            layout_presence_visible: true,
            layout_transition_version: 0,
            layout_shared: cx.new(|cx| {
                let mut state = SharedElementState::new(cx);
                state.set_source_bounds(Bounds::new(
                    point(px(14.0), px(54.0)),
                    size(px(124.0), px(74.0)),
                ));
                state.set_target_bounds(Bounds::new(
                    point(px(210.0), px(82.0)),
                    size(px(130.0), px(96.0)),
                ));
                state
            }),
            layout_shared_at_target: false,
            layout_sortable: cx.new(|_| {
                SortableListState::new(vec![
                    "Keyboard review".into(),
                    "Visual QA".into(),
                    "Release notes".into(),
                ])
            }),
            layout_infinite: InfiniteScrollState::new(cx),
            layout_infinite_count: 16,
            layout_drawer: cx.new(|_| DrawerState::new()),
            layout_drag_drop: cx.new(|_| DragDropKeyboardState::new()),
            layout_last_drop: "Nothing dropped yet".into(),
            effects_confetti: cx.new(ConfettiState::new),
            effects_aurora: cx.new(AuroraState::new_paused),
            effects_meteors: cx.new(MeteorState::new_paused),
            effects_particles: cx.new(|cx| {
                ParticleEmitterState::with_config(
                    ParticleEmitterConfig {
                        spawn_rate: 24.0,
                        lifetime: std::time::Duration::from_millis(1900),
                        velocity_range: (55.0, 125.0),
                        size_range: (3.0, 7.0),
                        color_start: hsla(0.55, 0.85, 0.62, 0.95),
                        color_end: hsla(0.76, 0.78, 0.62, 0.0),
                        gravity: 48.0,
                        spread_angle: std::f32::consts::PI * 0.72,
                        max_particles: 160,
                        origin: Point { x: 240.0, y: 190.0 },
                    },
                    cx,
                )
            }),
            effects_spotlight: cx.new(|_| SpotlightState::new()),
            effects_tilt: cx.new(TiltCardState::new),
            effects_dock: cx.new(DockState::new),
            effects_drag: cx.new(DraggableSpringState::new),
            effects_magnetic: cx.new(MagneticButtonState::new),
            effects_crop: cx.new(CropAreaState::new),
            effects_ripple_version: 0,
            page_scroll,
            category,
            show_dialog: overlay_open.as_deref() == Some("dialog"),
            show_sheet: overlay_open.as_deref() == Some("sheet"),
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
        .gap(px(18.0))
        .p(px(20.0))
        .bg(theme.tokens.card)
        .border_1()
        .border_color(theme.tokens.border)
        .rounded(theme.tokens.radius_lg)
        .shadow(theme.tokens.shadow_xs.to_vec())
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
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

fn demo_surface(theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .items_stretch()
        .min_w(px(0.0))
        .p(px(16.0))
        .bg(theme.tokens.background)
        .border_1()
        .border_color(theme.tokens.border)
        .rounded(theme.tokens.radius_lg)
}

fn label_chip(text: &str, theme: &Theme) -> Div {
    div()
        .text_size(px(11.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.tokens.muted_foreground)
        .child(text.to_string())
}

fn theme_pill<T: Fn(&mut App) + 'static>(
    label: &str,
    active: bool,
    tokens: ThemeTokens,
    theme: &Theme,
    on: T,
) -> impl IntoElement + use<T> {
    let swatch = tokens.primary;
    let theme = theme.clone();
    let label_text: SharedString = label.to_string().into();
    kael::button(SharedString::from(format!("theme-{label}")))
        .label(label_text.clone())
        .on_click(move |_, _, cx| on(cx))
        .render_with(move |state, _, _| {
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .h(px(32.0))
                .px(px(12.0))
                .rounded(theme.tokens.radius_md)
                .border_1()
                .border_color(if active {
                    theme.tokens.ring
                } else {
                    theme.tokens.border
                })
                .bg(if active {
                    theme.tokens.accent
                } else {
                    transparent_black()
                })
                .when(state.focused, |this| {
                    this.shadow(smallvec::smallvec![astryx_focus_ring_outer(
                        theme.tokens.ring,
                    )])
                })
                .hover(|style| style.bg(theme.tokens.accent))
                .child(div().size(px(12.0)).rounded_full().bg(swatch))
                .child(
                    div()
                        .accessibility(
                            AccessibilityAttributes::new(AccessibilityRole::Group)
                                .states(AccessibilityState::HIDDEN),
                        )
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.tokens.foreground)
                        .child(label_text.clone()),
                )
                .into_any_element()
        })
}

impl Render for AstryxShowcase {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = use_theme();
        let variant = theme.variant;
        let view = cx.entity();

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(20.0))
            .px(px(24.0))
            .py(px(16.0))
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
                        .icon("plus")
                        .tooltip("Create new"),
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

        let alert_retry_view = view.clone();
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
                    .description("We couldn't reach the server. Try again.")
                    .action("Try again", move |window, cx| {
                        alert_retry_view.update(cx, |this, cx| {
                            this.toast_n += 1;
                            let id = this.toast_n;
                            this.toasts.update(cx, |manager, cx| {
                                manager.add_toast(
                                    ToastItem::new(id, "Retry started")
                                        .description("Reconnecting to the server.")
                                        .variant(ToastVariant::Default),
                                    window,
                                    cx,
                                );
                            });
                        });
                    })
                    .dismissible(true),
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
                            .label("Free plan")
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
                            .label("Pro plan")
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
                .child(Spinner::new().size(SpinnerSize::Md).animation_cycles(2))
                .child(
                    Spinner::new()
                        .size(SpinnerSize::Md)
                        .variant(SpinnerVariant::Primary)
                        .animation_cycles(2),
                )
                .child(
                    Spinner::new()
                        .size(SpinnerSize::Md)
                        .shade(SpinnerShade::Subtle)
                        .animation_cycles(2),
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

        let _details = section(
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
                            .child(
                                IconButton::new("chevron-left")
                                    .label("Previous month")
                                    .size(px(28.0)),
                            )
                            .child(
                                IconButton::new("chevron-right")
                                    .label("Next month")
                                    .size(px(28.0)),
                            ),
                    ),
            ),
        );

        let more_inputs = section("More inputs", "Free-text field and icon buttons", &theme)
            .child(
                col().child(label_chip("Description", &theme)).child(
                    div().w(px(360.0)).child(
                        Textarea::new("showcase-description")
                            .value(self.field_textarea.clone())
                            .placeholder("Write a description...")
                            .rows(4)
                            .max_length(280)
                            .on_change({
                                let view = view.clone();
                                move |value, _, cx| {
                                    view.update(cx, |this, cx| {
                                        this.field_textarea = value;
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
                ),
            )
            .child(
                row()
                    .child(IconButton::new("search").label("Search"))
                    .child(IconButton::new("settings").label("Settings"))
                    .child(IconButton::new("plus").label("Add item")),
            );

        let primary_tabs_view = view.clone();
        let period_tabs_view = view.clone();
        let mail_tabs_view = view.clone();
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
                    .selected_index(self.nav_tab_primary)
                    .on_change(move |index, _, cx| {
                        primary_tabs_view.update(cx, |this, cx| {
                            this.nav_tab_primary = *index;
                            cx.notify();
                        });
                    }),
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
                    .selected_index(self.nav_tab_period)
                    .on_change(move |index, _, cx| {
                        period_tabs_view.update(cx, |this, cx| {
                            this.nav_tab_period = *index;
                            cx.notify();
                        });
                    }),
            );
        let nav_sec = nav_sec.child(
            TabList::new()
                .size(TabListSize::Sm)
                .layout(TabListLayout::Hug)
                .variant(TabVariant::Enclosed)
                .tab("inbox", "Inbox")
                .tab("sent", "Sent")
                .tab("archive", "Archive")
                .selected_id(self.nav_tab_mail.clone())
                .on_change(move |id, _, cx| {
                    mail_tabs_view.update(cx, |this, cx| {
                        this.nav_tab_mail = id.clone();
                        cx.notify();
                    });
                }),
        );

        let mut table_members = vec![
            ["Augustus Otu", "Owner", "Active"],
            ["Kael UI", "Editor", "Active"],
            ["Astryx", "Viewer", "Invited"],
        ];
        table_members
            .sort_by(|left, right| left[self.table_sort_column].cmp(right[self.table_sort_column]));
        if self.table_sort_direction == TableSortDirection::Descending {
            table_members.reverse();
        }
        let table_rows = table_members
            .into_iter()
            .map(|row| TableRow::new(row.into_iter().map(SharedString::from).collect()))
            .collect();
        let table_sort_view = view.clone();
        let data_table_section = section(
            "Data tables",
            "Production data controls and composable table primitives",
            &theme,
        )
            .child(
                col()
                    .gap(px(6.0))
                    .child(label_chip(
                        "DataTable — search, sort, select, resize and row actions",
                        &theme,
                    ))
                    .child(muted(
                        "Search across records, sort from the headers, select rows, and right-click for actions.",
                    ))
                    .child(self.production_data_table.clone()),
            )
            .child(
                col().child(label_chip("Sortable members", &theme)).child(
                    Table::new()
                        .id("showcase-members-table")
                        .columns(vec![
                            TableColumn::new("Name")
                                .column_width(proportional(1.0))
                                .sortable(true),
                            TableColumn::new("Role")
                                .column_width(pixel(px(110.0)))
                                .sortable(true),
                            TableColumn::new("Status")
                                .width(px(110.0))
                                .align(TableColumnAlign::Center)
                                .sortable(true),
                        ])
                        .sort(self.table_sort_column, self.table_sort_direction)
                        .on_sort(move |column, direction, _, cx| {
                            table_sort_view.update(cx, |this, cx| {
                                this.table_sort_column = column;
                                this.table_sort_direction = direction;
                                cx.notify();
                            });
                        })
                        .vertical_align(TableVerticalAlign::Middle)
                        .rows(table_rows),
                ),
            )
            .child(
                col()
                    .child(label_chip("Composable header, body and footer", &theme))
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
                                            .cell(
                                                TableCell::new(Badge::new("Done")).width(px(120.0)),
                                            )
                                            .cell(
                                                TableCell::new("Kael")
                                                    .width(px(140.0))
                                                    .align(TableColumnAlign::End),
                                            ),
                                    )
                                    .child(
                                        TableRow::children()
                                            .cell(TableCell::new("Table").width(px(160.0)))
                                            .cell(
                                                TableCell::new(Badge::new("Parity"))
                                                    .width(px(120.0)),
                                            )
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
                    ),
            )
            .child(
                col().child(label_chip("Empty state", &theme)).child(
                    Table::new()
                        .id("showcase-empty-table")
                        .columns(vec![
                            TableColumn::new("Name").column_width(proportional(1.0)),
                            TableColumn::new("Role").width(px(140.0)),
                        ])
                        .empty_content(
                            col()
                                .items_center()
                                .gap(px(4.0))
                                .child(h5("No members yet"))
                                .child(muted("Invite a teammate to populate this table.")),
                        ),
                ),
            );

        let rich_content_blocks = vec![
            RichBlock::Heading {
                level: 3,
                content: vec![RichInline::Text("Release readiness".into())],
            },
            RichBlock::Paragraph(vec![
                RichInline::Text("Astryx combines ".into()),
                RichInline::Bold(vec![RichInline::Text("structured content".into())]),
                RichInline::Text(" with ".into()),
                RichInline::Code("accessible desktop controls".into()),
                RichInline::Text(" without flattening semantics into plain text.".into()),
            ]),
            RichBlock::UnorderedList {
                items: vec![
                    RichListItem {
                        checked: Some(true),
                        content: vec![RichInline::Text("Keyboard interaction reviewed".into())],
                        children: vec![],
                    },
                    RichListItem {
                        checked: Some(true),
                        content: vec![RichInline::Text("Responsive layout verified".into())],
                        children: vec![],
                    },
                    RichListItem {
                        checked: Some(false),
                        content: vec![RichInline::Text("Assistive-technology lab pass".into())],
                        children: vec![],
                    },
                ],
            },
            RichBlock::BlockQuote(vec![RichBlock::Paragraph(vec![RichInline::Text(
                "Good data display keeps both the information and its structure readable.".into(),
            )])]),
        ];
        let rich_content_section = section(
            "Rich content",
            "Structured headings, inline emphasis, code, task lists and quotations",
            &theme,
        )
        .child(
            div()
                .p(px(16.0))
                .bg(theme.tokens.background)
                .border_1()
                .border_color(theme.tokens.border)
                .rounded(theme.tokens.radius_md)
                .children(render_blocks(
                    &rich_content_blocks,
                    px(14.0),
                    &None,
                    "showcase-rich-content",
                )),
        );
        let markdown_link_view = view.clone();
        let html_link_view = view.clone();
        let source_renderer_section = section(
            "Markdown & HTML",
            "Optional source renderers backed by the same structured rich-content system",
            &theme,
        )
        .child(muted(self.dd_source_link_status.clone()))
        .child(
            Grid::new()
                .columns(2)
                .gap(px(12.0))
                .child(
                    demo_surface(&theme)
                        .gap(px(12.0))
                        .child(label_chip("Markdown source", &theme))
                        .child(Markdown::new(
                            "### Audit notes\n\n- Accessible structure\n- **Strong** emphasis\n- `inline code`\n\n[Review Markdown docs](https://example.com/markdown)",
                        ).on_link_click(move |url, _, cx| {
                            markdown_link_view.update(cx, |this, cx| {
                                this.dd_source_link_status =
                                    format!("Markdown handler received: {url}").into();
                                cx.notify();
                            });
                        })),
                )
                .child(
                    demo_surface(&theme)
                        .gap(px(12.0))
                        .child(label_chip("Sanitized HTML source", &theme))
                        .child(Html::new(
                            "<h3>Release status</h3><p><strong>Stable</strong> across supported desktop targets. <a href=\"https://example.com/html\">Review HTML docs</a>.</p><blockquote>Structure remains readable.</blockquote>",
                        ).on_link_click(move |url, _, cx| {
                            html_link_view.update(cx, |this, cx| {
                                this.dd_source_link_status =
                                    format!("HTML handler received: {url}").into();
                                cx.notify();
                            });
                        })),
                ),
        );

        let data_grid_section = section(
            "Interactive data grid",
            "Keyboard navigation, sorting, resizing and inline editing",
            &theme,
        )
        .child(
            col()
                .gap(px(6.0))
                .child(muted(
                    "Select with the arrow keys. Double-click text or budget cells to edit; double-click Active to toggle.",
                ))
                .child(
                    DataGrid::new(self.data_grid.clone())
                        .id("showcase-project-data-grid")
                        .striped(true)
                        .h(px(176.0)),
                ),
        );

        let power_search_filter_view = view.clone();
        let power_search_query = self.dd_power_query.clone();
        let power_search_section = section(
            "Structured search",
            "Editable query, removable filters and discoverable field shortcuts",
            &theme,
        )
        .child(
            col()
                .gap(px(8.0))
                .child(
                    PowerSearch::new()
                        .id("showcase-power-search")
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
                        .filters(self.dd_power_filters.clone())
                        .query_state(&self.dd_power_query)
                        .on_filter_remove(move |index, _, cx| {
                            power_search_filter_view.update(cx, |this, cx| {
                                if index < this.dd_power_filters.len() {
                                    this.dd_power_filters.remove(index);
                                    cx.notify();
                                }
                            });
                        })
                        .on_field_select(move |field, window, cx| {
                            power_search_query.update(cx, |state, cx| {
                                state.set_value(format!("{}:", field.key), window, cx);
                            });
                        }),
                )
                .child(muted(
                    "Type a query, clear it, remove a filter, or choose a field to begin a structured expression.",
                )),
        );

        let chat_view = view.clone();
        let chat_composer = self.chat_composer.clone();
        let _parity_surfaces = section(
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
        .child(
            TopNavHeading::new("Kael UI")
                .superheading("ASTRYX parity")
                .logo("sparkles"),
        )
        .child(
            Layout::new()
                .header(
                    LayoutHeader::new(
                        div().child(
                            Heading::new("Layout frame")
                                .level(HeadingLevel::H4)
                                .heading_type(HeadingType::Display3),
                        ),
                    )
                    .has_divider(true),
                )
                .panel(
                    LayoutPanel::new(
                        div().p(px(8.0)).child(
                            SideNav::new()
                                .items(vec![
                                    SideNavItem::new("home".into(), "Home").with_icon("home"),
                                    SideNavItem::new("teams".into(), "Teams").with_icon("users"),
                                    SideNavItem::new("billing".into(), "Billing")
                                        .with_icon("credit-card"),
                                ])
                                .selected_id("home"),
                        ),
                    )
                    .width(px(180.0))
                    .has_divider(true),
                )
                .content(
                    LayoutContent::new(
                        div()
                            .child(
                                row()
                                    .gap(px(8.0))
                                    .items_center()
                                    .child(
                                        MobileNavToggle::new()
                                            .open(true)
                                            .label("Toggle navigation"),
                                    )
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
                                            .item(
                                                NavItem::new("Inbox").icon("inbox").selected(true),
                                            )
                                            .item(NavItem::new("Archive").icon("archive")),
                                    )
                                    .content(
                                        div()
                                            .p(px(12.0))
                                            .child(
                                                InteractiveRoleContext::new(
                                                    InteractiveRole::Button,
                                                )
                                                .child(
                                                    Item::new("Focusable action")
                                                        .icon("mouse-pointer"),
                                                ),
                                            )
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
                    )
                    .padding(px(12.0)),
                )
                .gap(px(0.0))
                .h(px(420.0))
                .w_full()
                .border_1()
                .border_color(theme.tokens.border)
                .rounded(theme.tokens.radius_xl)
                .overflow_hidden(),
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
                .child(StackItem::new(
                    Button::new("stack-action", "Action").size(ButtonSize::Sm),
                )),
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
                                                "contains", "contains",
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
                            .nodes(vec![
                                TreeNode::new(SharedString::from("src"), "src")
                                    .with_icon("folder")
                                    .with_children(vec![
                                        TreeNode::new(
                                            SharedString::from("components"),
                                            "components",
                                        )
                                        .with_icon("folder-open"),
                                        TreeNode::new(SharedString::from("theme"), "theme")
                                            .with_icon("palette"),
                                    ]),
                            ]),
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
                .child(div().w(px(360.0)).child(CheckboxList::new().items(vec![
                            CheckboxListItem::new("api", "API parity")
                                .description("Named ASTRYX surfaces are exported")
                                .checked(true),
                            CheckboxListItem::new("visual", "Visual review")
                                .description("Rendered comparison still required"),
                        ])))
                .child(
                    div().w(px(220.0)).child(
                        Divider::new()
                            .variant(DividerVariant::Strong)
                            .label("strong"),
                    ),
                ),
        )
        .child(
            CollapsibleGroup::new()
                .child(
                    Collapsible::new()
                        .label("Astryx API names")
                        .trigger(body("ASTRYX API names".to_string()))
                        .content(muted(
                            "Divider, InputGroupText and CollapsibleGroup are now public.",
                        ))
                        .open(true),
                )
                .child(SideNavHeading::new("Navigation aliases"))
                .child(
                    Collapsible::new()
                        .label("Visual details")
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
        .child(row().items_start().child(div().w(px(360.0)).child({
            let selected = self.dd_outline_selected.clone();
            let outline_view = view.clone();
            Outline::new()
                .items(vec![
                    OutlineItem::new("inputs", "Inputs").active(selected == "inputs"),
                    OutlineItem::new("tables", "Tables")
                        .level(1)
                        .active(selected == "tables"),
                    OutlineItem::new("navigation", "Navigation")
                        .level(1)
                        .active(selected == "navigation"),
                    OutlineItem::new("overlays", "Overlays")
                        .level(1)
                        .active(selected == "overlays"),
                ])
                .on_select(move |id, _, cx| {
                    outline_view.update(cx, |this, cx| {
                        this.dd_outline_selected = id;
                        cx.notify();
                    });
                })
        })))
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
                            .id("parity-chat")
                            .messages(self.chat_messages.clone())
                            .composer_state(&self.chat_composer)
                            .on_submit(move |value, cx| {
                                let body = value.trim();
                                if body.is_empty() {
                                    return;
                                }
                                chat_view.update(cx, |this, cx| {
                                    let index = this.chat_messages.len() + 1;
                                    this.chat_messages.push(
                                        ChatMessage::new(format!("user-{index}"), body.to_owned())
                                            .role(ChatMessageRole::User)
                                            .author("You")
                                            .timestamp("Now"),
                                    );
                                    cx.notify();
                                });
                                chat_composer.update(cx, |state, cx| state.clear(cx));
                            }),
                    ),
                )
                .child(
                    div().w(px(320.0)).child(
                        ClickableCard::new().selected(true).child(
                            col()
                                .child(
                                    Code::new("TableDensity::Compact").variant(CodeVariant::Inline),
                                )
                                .child(Link::new("Open upstream reference").external(true)),
                        ),
                    ),
                ),
        );

        let timeline_sec = section("Timeline", "Activity feed", &theme).child(Timeline::new(vec![
            TimelineItem::new("Project created").description("Repository initialized"),
            TimelineItem::new("First release").description("v0.1.0 shipped"),
            TimelineItem::new("Astryx redesign").description("Components matched to Astryx"),
        ]));

        let empty_create_view = view.clone();
        let empty_import_view = view.clone();
        let empty_disclosure = section(
            "Empty state & collapsible",
            "Placeholders & disclosure",
            &theme,
        )
        .child(
            EmptyState::new("empty-demo", "No projects yet")
                .description("Create your first project to get started.")
                .icon("inbox")
                .action("Create project", move |window, cx| {
                    empty_create_view.update(cx, |this, cx| {
                        this.toast_n += 1;
                        let id = this.toast_n;
                        this.toasts.update(cx, |manager, cx| {
                            manager.add_toast(
                                ToastItem::new(id, "Project creation opened")
                                    .variant(ToastVariant::Success),
                                window,
                                cx,
                            );
                        });
                    });
                })
                .secondary_action("Import", move |window, cx| {
                    empty_import_view.update(cx, |this, cx| {
                        this.toast_n += 1;
                        let id = this.toast_n;
                        this.toasts.update(cx, |manager, cx| {
                            manager.add_toast(
                                ToastItem::new(id, "Import workflow opened")
                                    .variant(ToastVariant::Default),
                                window,
                                cx,
                            );
                        });
                    });
                }),
        )
        .child(
            Collapsible::new()
                .label("Advanced settings")
                .trigger(body("Advanced settings".to_string()))
                .content(muted(
                    "These options are hidden until expanded.".to_string(),
                ))
                .default_open(true),
        );

        let dropdowns = section(
            "Select & number input",
            "Dropdown and stepper input",
            &theme,
        )
        .child(div().w(px(280.0)).child(self.select.clone()))
        .child(
            div().w(px(240.0)).child(
                NumberInput::new(self.number.clone())
                    .label("Quantity")
                    .description("Choose between 0 and 100 items")
                    .start_icon("hash")
                    .units("items")
                    .min(0.0, cx)
                    .max(100.0, cx)
                    .show_buttons(true)
                    .clearable(true)
                    .status(FieldStatusType::Success)
                    .status_message("Within inventory limits"),
            ),
        );

        let rating_stepper = section("Rating & stepper", "Feedback and multi-step flows", &theme)
            .child(Rating::new(self.rating.clone()))
            .child(Stepper::new(self.stepper.clone()));

        let otp_date = section("OTP & date picker", "Specialized inputs", &theme).child(
            row()
                .items_start()
                .gap(px(32.0))
                .child(
                    col()
                        .w(px(300.0))
                        .gap(px(16.0))
                        .child(OTPInput::new(&self.otp))
                        .child(
                            div()
                                .w(px(220.0))
                                .child(DatePicker::new(self.date.clone()).size(InputSize::Lg)),
                        ),
                )
                .child(
                    Calendar::new()
                        .id("inputs-calendar")
                        .label("Release date")
                        .current_month(self.inputs_calendar_month)
                        .selected_date(self.inputs_calendar_date)
                        .min("2026-06-01")
                        .max("2026-07-31")
                        .has_week_numbers(true)
                        .week_starts_on(DayOfWeek::MONDAY)
                        .on_date_select({
                            let calendar_view = view.clone();
                            move |date, _, cx| {
                                calendar_view.update(cx, |this, cx| {
                                    this.inputs_calendar_date = *date;
                                    cx.notify();
                                });
                            }
                        })
                        .on_month_change({
                            let calendar_view = view.clone();
                            move |date, _, cx| {
                                calendar_view.update(cx, |this, cx| {
                                    this.inputs_calendar_month =
                                        DateValue::new(date.year, date.month, 1);
                                    cx.notify();
                                });
                            }
                        }),
                ),
        );

        let pickers = section("Pickers", "Time, color and file upload", &theme)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_start()
                    .gap(px(12.0))
                    .child(
                        div().w(px(190.0)).child(
                            TimePicker::new(self.time_state.clone())
                                .label("24-hour time")
                                .description("Independent hour and minute columns")
                                .size(InputSize::Lg)
                                .clearable(true),
                        ),
                    )
                    .child(
                        div().w(px(190.0)).child(
                            TimePicker::new(self.time_12_state.clone())
                                .label("12-hour time")
                                .hour_format(TimeFormat::Hour12)
                                .size(InputSize::Lg),
                        ),
                    )
                    .child(
                        div().w(px(210.0)).child(
                            TimePicker::new(self.time_seconds_state.clone())
                                .label("Time with seconds")
                                .has_seconds(true)
                                .size(InputSize::Lg),
                        ),
                    )
                    .child(
                        div().w(px(190.0)).child(
                            TimePicker::new(self.time_read_only_state.clone())
                                .label("Read-only time")
                                .description("Available for review, but cannot be changed")
                                .read_only(true)
                                .size(InputSize::Lg),
                        ),
                    ),
            )
            .child(
                row()
                    .items_start()
                    .gap(px(24.0))
                    .child(
                        col()
                            .w(px(220.0))
                            .child(label_chip("Color", &theme))
                            .child(ColorPicker::new("cp-demo", self.color_state.clone())),
                    )
                    .child(
                        col()
                            .flex_1()
                            .min_w(px(280.0))
                            .child(label_chip("Files", &theme))
                            .child(
                                FileUpload::new("fu-demo", self.file_state.clone())
                                    .multiple(true)
                                    .accept_images()
                                    .max_file_size_mb(10),
                            ),
                    ),
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
                            .trigger_button("Open popover")
                            .trigger_button_variant(ButtonVariant::Outline)
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
                                    .accessibility_label("Quick actions")
                                })
                            }),
                    ),
            )
            .child(
                row()
                    .items_start()
                    .flex_wrap()
                    .gap(px(16.0))
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
                                    .item(
                                        ContextMenuItem::new("open", "Open").icon("external-link"),
                                    )
                                    .item(ContextMenuItem::new("copy", "Copy link").icon("copy"))
                                    .item(ContextMenuItem::separator())
                                    .item(
                                        ContextMenuItem::new("delete", "Delete")
                                            .icon("trash")
                                            .destructive(true),
                                    ),
                            ),
                    ),
            );

        let overlay_trigger_audit = std::env::var("ASTRYX_SHOWCASE_OVERLAY_TRIGGER").ok();
        let modal_triggers = section(
            "Overlays — click to open",
            "Dialog, sheet and toast",
            &theme,
        )
        .when(
            overlay_trigger_audit
                .as_deref()
                .is_none_or(|section| section == "buttons"),
            |this| {
                this.child(
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
                            Button::new("open-bottom-sheet", "Open bottom sheet")
                                .variant(ButtonVariant::Outline)
                                .on_click({
                                    let view = view.clone();
                                    move |_, _, cx| {
                                        view.update(cx, |this, cx| {
                                            this.show_bottom_sheet = true;
                                            cx.notify();
                                        });
                                    }
                                }),
                        )
                        .child(
                            Button::new("open-popover-menu", "Open popover menu")
                                .variant(ButtonVariant::Outline)
                                .on_click({
                                    let view = view.clone();
                                    move |event, _, cx| {
                                        view.update(cx, |this, cx| {
                                            this.show_popover_menu = true;
                                            this.popover_menu_position =
                                                event.position() + point(px(0.0), px(8.0));
                                            cx.notify();
                                        });
                                    }
                                }),
                        )
                        .child(
                            Button::new("open-command-palette", "Open command palette")
                                .variant(ButtonVariant::Outline)
                                .on_click({
                                    let view = view.clone();
                                    move |_, _, cx| {
                                        view.update(cx, |this, cx| {
                                            this.show_command_palette = true;
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
            },
        )
        .when(
            overlay_trigger_audit
                .as_deref()
                .is_none_or(|section| section == "inline-toasts"),
            |this| {
                this.child(
                    row()
                        .items_start()
                        .child(
                            Toast::new("Inline Toast mirrors the ASTRYX preview API.")
                                .toast_type(ToastType::Info)
                                .end_content(Badge::new("Info").variant(BadgeVariant::Info)),
                        )
                        .child(
                            Toast::new("Error toasts use assertive destructive styling.")
                                .toast_type(ToastType::Error)
                                .end_content(Badge::new("Error").variant(BadgeVariant::Error)),
                        ),
                )
            },
        )
        .when(
            overlay_trigger_audit
                .as_deref()
                .is_none_or(|section| section == "viewport"),
            |this| {
                this.child(
                    ToastViewport::new()
                        .position(ToastPosition::BottomEnd)
                        .max_visible(3)
                        .child(label("ToastViewport wraps app content.".to_string())),
                )
            },
        );

        let removable_token_view = view.clone();
        let code_toggle_view = view.clone();
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
                    .when(self.dd_show_removable_token, |tokens| {
                        tokens.child(Token::new("Removable").color(TokenColor::Blue).on_remove(
                            move |_, cx| {
                                removable_token_view.update(cx, |this, cx| {
                                    this.dd_show_removable_token = false;
                                    cx.notify();
                                });
                            },
                        ))
                    })
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
                    .value(self.selection_toggle_value.clone())
                    .on_change(move |value, _, cx| {
                        code_toggle_view.update(cx, |this, cx| {
                            this.selection_toggle_value = value.clone();
                            cx.notify();
                        });
                    }),
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
                    .child(
                        IconButton::new("star")
                            .label("Favorite")
                            .variant(ButtonVariant::Default),
                    )
                    .child(
                        IconButton::new("heart")
                            .label("Like")
                            .variant(ButtonVariant::Secondary),
                    )
                    .child(
                        IconButton::new("trash")
                            .label("Delete")
                            .variant(ButtonVariant::Destructive),
                    )
                    .child(
                        IconButton::new("settings")
                            .label("Settings")
                            .variant(ButtonVariant::Outline),
                    )
                    .child(
                        IconButton::new("search")
                            .label("Search")
                            .variant(ButtonVariant::Ghost),
                    )
                    .child(IconButton::new("lock").label("Locked").disabled(true)),
            ),
        )
        .child(
            col().child(label_chip("Sizes", &theme)).child(
                row()
                    .child(
                        IconButton::new("plus")
                            .label("Add small")
                            .size(px(28.0))
                            .variant(ButtonVariant::Outline),
                    )
                    .child(
                        IconButton::new("plus")
                            .label("Add medium")
                            .size(px(32.0))
                            .variant(ButtonVariant::Outline),
                    )
                    .child(
                        IconButton::new("plus")
                            .label("Add large")
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
                        .icon("plus")
                        .size(FABSize::Md)
                        .action("compose", "pencil", |_, _| {})
                        .action("upload", "upload", |_, _| {})
                        .action("share", "share-2", |_, _| {}),
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
                div().w(px(320.0)).child(
                    RangeSlider::new(self.inputs_range.clone())
                        .id("showcase-range-default")
                        .show_values(true),
                ),
            ),
        )
        .child(
            col().child(label_chip("Large", &theme)).child(
                div().w(px(320.0)).child(
                    RangeSlider::new(self.inputs_range_large.clone())
                        .id("showcase-range-large")
                        .size(SliderSize::Lg),
                ),
            ),
        )
        .child(
            col().child(label_chip("Disabled", &theme)).child(
                div().w(px(320.0)).child(
                    RangeSlider::new(self.inputs_range_disabled.clone())
                        .id("showcase-range-disabled")
                        .disabled(true),
                ),
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
                            .search_source([
                                TokenizerItem::new("grace", "Grace Hopper"),
                                TokenizerItem::new("katherine", "Katherine Johnson"),
                                TokenizerItem::new("margaret", "Margaret Hamilton"),
                            ])
                            .entries_on_focus(true)
                            .creatable(true)
                            .max_entries(5)
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
                .child(div().w(px(360.0)).child({
                    let selected = self.inputs_typeahead_value.clone();
                    let selected_label = selected.as_ref().map(|item| item.label.clone());
                    let change_view = view.clone();
                    let query_view = view.clone();
                    let clear_view = view.clone();
                    Typeahead::new("Assignee")
                        .placeholder("Search teammates...")
                        .search_source(SearchSource::new([
                            SearchableItem::new("ada", "Ada Lovelace"),
                            SearchableItem::new("alan", "Alan Turing"),
                            SearchableItem::new("grace", "Grace Hopper"),
                        ]))
                        .query(self.inputs_typeahead_query.clone())
                        .when_some(selected, |this, selected| this.value(selected))
                        .entries_on_focus(true)
                        .clearable(true)
                        .on_change(move |item, _, cx| {
                            change_view.update(cx, |this, cx| {
                                this.inputs_typeahead_value = Some(item);
                                this.inputs_typeahead_query = SharedString::default();
                                cx.notify();
                            });
                        })
                        .on_change_query(move |query, _, cx| {
                            query_view.update(cx, |this, cx| {
                                if selected_label.as_ref() != Some(&query) {
                                    this.inputs_typeahead_value = None;
                                }
                                this.inputs_typeahead_query = query;
                                cx.notify();
                            });
                        })
                        .on_clear(move |_, cx| {
                            clear_view.update(cx, |this, cx| {
                                this.inputs_typeahead_value = None;
                                this.inputs_typeahead_query = SharedString::default();
                                cx.notify();
                            });
                        })
                })),
        );

        let inputs_structured = section(
            "Structured form inputs",
            "Persistent text, date ranges, and paired date-time controls",
            &theme,
        )
        .child(
            FormLayout::new()
                .direction(FormLayoutDirection::Horizontal)
                .columns(2)
                .child(
                    Field::new(
                        "Workspace name",
                        TextField::from_state(self.inputs_text_field.clone())
                            .label("Workspace name")
                            .placeholder("Name this workspace")
                            .w_full(),
                    )
                    .description("Persistent across parent re-renders")
                    .required(true),
                )
                .child(
                    DateRangeInput::new("Release window", self.inputs_date_range.clone())
                        .description("Choose the first and last active day")
                        .weekends_disabled(true)
                        .required(true),
                ),
        )
        .child(
            DateTimeInput::new(
                "Publish at",
                self.inputs_datetime_date.clone(),
                self.inputs_datetime_time.clone(),
            )
            .description("Date and time retain independent focus and selection")
            .clearable(true)
            .required(true),
        );

        let inputs_editing = section(
            "Editing, shortcuts & mentions",
            "Keyboard-first controls with durable editing state",
            &theme,
        )
        .child(
            FormLayout::new()
                .direction(FormLayoutDirection::Horizontal)
                .columns(2)
                .child(
                    col()
                        .child(label_chip("Hotkey input", &theme))
                        .child(
                            HotkeyInput::new(self.inputs_hotkey.clone())
                                .label("Open command palette")
                                .placeholder("Record a shortcut"),
                        )
                        .child(caption(
                            "Activate, then press a combination. Escape cancels recording.",
                        )),
                )
                .child(
                    col()
                        .child(label_chip("Inline edit", &theme))
                        .child(
                            div()
                                .min_h(px(40.0))
                                .flex()
                                .items_center()
                                .px(px(12.0))
                                .bg(theme.tokens.background)
                                .border_1()
                                .border_color(theme.tokens.border)
                                .rounded(theme.tokens.radius_md)
                                .child(
                                    InlineEdit::new(self.inputs_inline_edit.clone())
                                        .placeholder("Add a project title"),
                                ),
                        )
                        .child(caption("Click to edit; Enter saves and Escape cancels.")),
                ),
        )
        .child(
            col()
                .child(label_chip("Mention input", &theme))
                .child(
                    div().w_full().child(
                        MentionInput::new(
                            &self.inputs_mention,
                            vec![
                                MentionItem::new("ada", "Ada Lovelace"),
                                MentionItem::new("grace", "Grace Hopper"),
                                MentionItem::new("katherine", "Katherine Johnson"),
                                MentionItem::new("margaret", "Margaret Hamilton"),
                            ],
                        )
                        .placeholder("Assign a reviewer with @"),
                    ),
                )
                .child(caption(
                    "Type @, filter by name, then use Arrow keys and Enter to select.",
                )),
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
        .child({
            let selected_assignees = self.selection_assignees.clone();
            let open_assignees = view.clone();
            let clear_assignees = view.clone();
            let open_labels = view.clone();
            col()
                .child(
                    MultiSelector::new("Assignees")
                        .description("Pick teammates for this task")
                        .placeholder("Select assignees")
                        .start_icon("users")
                        .clearable(true)
                        .max_visible(2)
                        .options(vec![
                            MultiSelectorOption::new("Ada Lovelace", "ada")
                                .selected(selected_assignees.iter().any(|id| id == "ada")),
                            MultiSelectorOption::new("Alan Turing", "alan")
                                .selected(selected_assignees.iter().any(|id| id == "alan")),
                            MultiSelectorOption::new("Grace Hopper", "grace")
                                .selected(selected_assignees.iter().any(|id| id == "grace")),
                            MultiSelectorOption::new("Edsger Dijkstra", "edsger"),
                        ])
                        .on_open(move |window, cx| {
                            open_assignees.update(cx, |this, cx| {
                                this.toast_n += 1;
                                let id = this.toast_n;
                                this.toasts.update(cx, |manager, cx| {
                                    manager.add_toast(
                                        ToastItem::new(id, "Assignee picker opened").description(
                                            "The trigger is ready to host a custom picker.",
                                        ),
                                        window,
                                        cx,
                                    );
                                });
                            });
                        })
                        .on_clear(move |window, cx| {
                            clear_assignees.update(cx, |this, cx| {
                                this.selection_assignees.clear();
                                this.toast_n += 1;
                                let id = this.toast_n;
                                this.toasts.update(cx, |manager, cx| {
                                    manager.add_toast(
                                        ToastItem::new(id, "Assignees cleared")
                                            .variant(ToastVariant::Success),
                                        window,
                                        cx,
                                    );
                                });
                                cx.notify();
                            });
                        }),
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
                        ])
                        .on_open(move |window, cx| {
                            open_labels.update(cx, |this, cx| {
                                this.toast_n += 1;
                                let id = this.toast_n;
                                this.toasts.update(cx, |manager, cx| {
                                    manager.add_toast(
                                        ToastItem::new(id, "Label picker opened"),
                                        window,
                                        cx,
                                    );
                                });
                            });
                        }),
                )
        });

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
                    .child(
                        QRCodeComponent::new("https://github.com/kael")
                            .label("QR code for the Kael GitHub repository")
                            .size(px(120.0)),
                    )
                    .child(
                        QRCodeComponent::new("kael://launch")
                            .label("QR code for the Kael launch link")
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

        let media_interactive_effects = section(
            "Interactive Visual Surfaces",
            "Pointer-aware depth, direct manipulation and production media tools",
            &theme,
        )
        .child(
            row()
                .items_stretch()
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .min_w(px(0.0))
                        .child(label_chip("Spotlight", &theme))
                        .child(
                            Spotlight::new(
                                "showcase-spotlight",
                                self.effects_spotlight.clone(),
                            )
                            .size(px(240.0))
                            .intensity(0.18)
                            .h(px(190.0))
                            .w_full()
                            .rounded(theme.tokens.radius_lg)
                            .bg(hsla(0.63, 0.3, 0.12, 1.0))
                            .child(
                                col()
                                    .size_full()
                                    .items_center()
                                    .justify_center()
                                    .gap(px(8.0))
                                    .text_color(white())
                                    .child(Icon::new("search").size(IconSize::Lg))
                                    .child(
                                        Heading::new("Follow the pointer")
                                            .level(HeadingLevel::H4)
                                            .color(white()),
                                    )
                                    .child(
                                        caption("Local coordinates keep the glow contained")
                                            .text_color(TextColor::Inherit),
                                    ),
                            ),
                        ),
                )
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .min_w(px(0.0))
                        .child(label_chip("TiltCard", &theme))
                        .child(
                            TiltCard::new("showcase-tilt-card", self.effects_tilt.clone())
                                .intensity(0.9)
                                .h(px(190.0))
                                .w_full()
                                .p(px(18.0))
                                .child(
                                    col()
                                        .size_full()
                                        .justify_between()
                                        .child(
                                            row()
                                                .items_center()
                                                .justify_between()
                                                .child(Badge::new("Interactive"))
                                                .child(Icon::new("move-3d").size(IconSize::Md)),
                                        )
                                        .child(
                                            col()
                                                .gap(px(4.0))
                                                .child(h4("Cursor-aware depth".to_string()))
                                                .child(muted(
                                                    "A restrained shadow shift—never a layout jump.",
                                                )),
                                        ),
                                ),
                        ),
                ),
        )
        .child(
            row()
                .items_stretch()
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .min_w(px(0.0))
                        .child(label_chip("Dock", &theme))
                        .child(
                            div()
                                .h(px(140.0))
                                .w_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(theme.tokens.radius_lg)
                                .bg(theme.tokens.muted)
                                .child(
                                    Dock::new("showcase-dock", self.effects_dock.clone())
                                        .max_scale(0.42)
                                        .item_size(px(44.0))
                                        .gap(px(5.0))
                                        .child(
                                            IconButton::new("layout-dashboard")
                                                .label("Dashboard"),
                                        )
                                        .child(IconButton::new("search").label("Search"))
                                        .child(IconButton::new("bell").label("Notifications"))
                                        .child(IconButton::new("settings").label("Settings")),
                                ),
                        ),
                )
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .min_w(px(0.0))
                        .child(label_chip("MagneticButton & DraggableSpring", &theme))
                        .child(
                            div()
                                .relative()
                                .h(px(140.0))
                                .w_full()
                                .overflow_hidden()
                                .rounded(theme.tokens.radius_lg)
                                .border_1()
                                .border_color(theme.tokens.border)
                                .child(
                                    div()
                                        .absolute()
                                        .left(px(22.0))
                                        .top(px(48.0))
                                        .child(
                                            MagneticButton::new(
                                                "showcase-magnetic-button",
                                                self.effects_magnetic.clone(),
                                            )
                                            .strength(0.45)
                                            .range(px(80.0))
                                            .child(
                                                Button::new(
                                                    "showcase-magnetic-action",
                                                    "Hover gently",
                                                )
                                                .size(ButtonSize::Sm),
                                            ),
                                        ),
                                )
                                .child(
                                    DraggableSpring::new(
                                        "showcase-draggable-spring",
                                        self.effects_drag.clone(),
                                    )
                                    .label("Draggable review card")
                                    .snap_points(
                                        vec![point(0.0, 0.0), point(54.0, 0.0)],
                                        cx,
                                    )
                                    .absolute()
                                    .right(px(88.0))
                                    .top(px(45.0))
                                    .child(
                                        div()
                                            .px(px(12.0))
                                            .py(px(8.0))
                                            .rounded(theme.tokens.radius_md)
                                            .bg(theme.tokens.accent)
                                            .child(label("Drag me".to_string())),
                                    ),
                                ),
                        ),
                ),
        )
        .child(
            row()
                .items_stretch()
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .min_w(px(0.0))
                        .child(label_chip("CropArea", &theme))
                        .child(
                            CropArea::new("showcase-crop", self.effects_crop.clone())
                                .label("Fabric and antler crop selection")
                                .aspect_ratio(16.0 / 9.0)
                                .min_size(0.12)
                                .h(px(220.0))
                                .w_full()
                                .rounded(theme.tokens.radius_lg)
                                .child(
                                    div().absolute().inset_0().child(
                                        img("assets/images/carousel_1.jpg")
                                            .size_full()
                                            .object_fit(ObjectFit::Cover),
                                    ),
                                ),
                        ),
                )
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .min_w(px(0.0))
                        .child(label_chip("Noise", &theme))
                        .child(
                            Noise::new()
                                .density(0.16)
                                .opacity(0.09)
                                .grain_size(px(1.2))
                                .h(px(220.0))
                                .w_full()
                                .rounded(theme.tokens.radius_lg)
                                .bg(theme.tokens.muted)
                                .child(
                                    col()
                                        .size_full()
                                        .items_center()
                                        .justify_center()
                                        .gap(px(6.0))
                                        .child(h4("Tactile surface".to_string()))
                                        .child(muted("Subtle texture, bounded rendering cost")),
                                ),
                        ),
                ),
        )
        .child(
            demo_surface(&theme)
                .w_full()
                .child(label_chip("SVGRenderer — relative paths & arcs", &theme))
                .child(
                    row()
                        .items_center()
                        .gap(px(18.0))
                        .child(
                            SVGRenderer::new()
                                .path_data("M11.017 2.814a1 1 0 0 1 1.966 0l1.051 5.558a2 2 0 0 0 1.594 1.594l5.558 1.051a1 1 0 0 1 0 1.966l-5.558 1.051a2 2 0 0 0-1.594 1.594l-1.051 5.558a1 1 0 0 1-1.966 0l-1.051-5.558a2 2 0 0 0-1.594-1.594l-5.558-1.051a1 1 0 0 1 0-1.966l5.558-1.051a2 2 0 0 0 1.594-1.594z")
                                .view_box(0.0, 0.0, 24.0, 24.0)
                                .no_fill()
                                .stroke(theme.tokens.primary)
                                .stroke_width(1.8)
                                .size(px(88.0)),
                        )
                        .child(
                            col()
                                .gap(px(4.0))
                                .child(h4("Native icon geometry".to_string()))
                                .child(muted(
                                    "A real Astryx asset path exercises relative lines and elliptical arcs.",
                                )),
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
                    demo_surface(&theme).w_full().min_w(px(0.0)).child(
                        BarChart::new(vec![
                            BarChartData::new("Jan", 120.0),
                            BarChartData::new("Feb", 200.0),
                            BarChartData::new("Mar", 150.0),
                            BarChartData::new("Apr", 280.0),
                            BarChartData::new("May", 190.0),
                        ])
                        .show_values(true)
                        .show_grid(true)
                        .chart_height(px(220.0)),
                    ),
                ),
        )
        .child(
            col().child(label_chip("Horizontal", &theme)).child(
                demo_surface(&theme).w_full().child(
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
                    demo_surface(&theme).w_full().child(
                        BarChart::multi_series(
                            vec!["Q1", "Q2", "Q3", "Q4"],
                            vec![
                                BarChartSeries::new("Revenue", vec![120.0, 180.0, 150.0, 240.0]),
                                BarChartSeries::new("Cost", vec![80.0, 110.0, 95.0, 140.0]),
                            ],
                        )
                        .show_grid(true)
                        .show_legend(true)
                        .chart_height(px(220.0)),
                    ),
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
                    demo_surface(&theme)
                        .w_full()
                        .min_w(px(0.0))
                        .h(px(280.0))
                        .child(
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
                            .h(px(248.0))
                            .x_labels(vec!["Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]),
                        ),
                ),
        )
        .child(
            col()
                .child(label_chip("Multi-series, smoothed, filled", &theme))
                .child(
                    demo_surface(&theme)
                        .w_full()
                        .min_w(px(0.0))
                        .h(px(280.0))
                        .child(
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
                            .h(px(248.0))
                            .smooth(true),
                        ),
                ),
        );

        let charts_area = section("Area Chart", "Overlaid and stacked filled areas", &theme)
            .child(
                col().child(label_chip("Overlaid (default)", &theme)).child(
                    demo_surface(&theme)
                        .id("chart-area-overlay-scroll")
                        .overflow_x_scroll()
                        .child(
                            AreaChart::new()
                                .size(AreaChartSize::Md)
                                .w_full()
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
                ),
            )
            .child(
                col().child(label_chip("Stacked", &theme)).child(
                    demo_surface(&theme)
                        .id("chart-area-stacked-scroll")
                        .overflow_x_scroll()
                        .child(
                            AreaChart::new()
                                .size(AreaChartSize::Md)
                                .w_full()
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
                ),
            );

        let charts_pie_donut = section(
            "Pie & Donut",
            "Proportional segments with legends and center labels",
            &theme,
        )
        .child(
            row()
                .items_start()
                .child(
                    col()
                        .flex_1()
                        .min_w(px(280.0))
                        .child(label_chip("Pie with legend + percentages", &theme))
                        .child(
                            PieChart::pie(vec![
                                PieChartSegment::new("Chrome", 62.0),
                                PieChartSegment::new("Safari", 19.0),
                                PieChartSegment::new("Firefox", 11.0),
                                PieChartSegment::new("Edge", 8.0),
                            ])
                            .size(PieChartSize::Md)
                            .segment_gap_degrees(1.5)
                            .label_position(PieChartLabelPosition::Legend)
                            .show_percentages(true),
                        ),
                )
                .child(
                    col()
                        .flex_1()
                        .min_w(px(280.0))
                        .child(label_chip("Donut with center value", &theme))
                        .child(
                            DonutChart::new()
                                .size(DonutChartSize::Md)
                                .segments(vec![
                                    PieChartSegment::new("Used", 68.0),
                                    PieChartSegment::new("Free", 32.0),
                                ])
                                .segment_gap_degrees(1.5)
                                .center_value("68%")
                                .center_label("Storage")
                                .show_legend(true)
                                .show_percentages(true),
                        ),
                ),
        );

        let charts_gauge = section("Gauge", "Semicircular progress indicators", &theme)
            .pb(px(48.0))
            .child(
                row()
                    .w_full()
                    .items_start()
                    .child(
                        demo_surface(&theme)
                            .w(px(184.0))
                            .max_w_full()
                            .items_center()
                            .child(label_chip("Small", &theme))
                            .child(
                                Gauge::new("gauge-cpu")
                                    .value(0.42)
                                    .label("CPU")
                                    .size(GaugeSize::Sm),
                            ),
                    )
                    .child(
                        demo_surface(&theme)
                            .w(px(264.0))
                            .max_w_full()
                            .items_center()
                            .child(label_chip("Medium", &theme))
                            .child(
                                Gauge::new("gauge-mem")
                                    .value(0.73)
                                    .label("Memory")
                                    .size(GaugeSize::Md),
                            ),
                    )
                    .child(
                        demo_surface(&theme)
                            .w(px(364.0))
                            .max_w_full()
                            .items_center()
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
                        .size(RadarChartSize::Lg)
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
                            .cell_size(px(64.0))
                            .show_values(true),
                    ),
            );

        let contribution_end = DateValue::today();
        let contribution_start = contribution_end.add_days(-364);
        let contribution_days: Vec<_> = (0..365_i64)
            .filter_map(|offset| {
                let date = contribution_start.add_days(offset);
                let weekday = date.day_of_week().index();
                let signal = ((offset * 17 + offset * offset * 3 + 11).rem_euclid(29)) as u32;
                let count = if weekday == 0 || (offset + i64::from(weekday)) % 9 == 0 {
                    0
                } else {
                    signal.saturating_sub(7) / 2
                };
                (count > 0).then_some(ContributionDay::new(date, count))
            })
            .collect();
        let charts_contributions = section(
            "Contribution Calendar",
            "GitHub-style daily activity with accessible summaries and palette presets",
            &theme,
        )
        .child(
            col()
                .child(label_chip(
                    "GitHub palette · rounded cells · full year",
                    &theme,
                ))
                .child(
                    demo_surface(&theme).child(
                        ContributionCalendar::new()
                            .date_range(contribution_start, contribution_end)
                            .contributions(contribution_days.clone())
                            .thresholds([2, 5, 8, 11])
                            .accessible_label("Repository contributions in the last year"),
                    ),
                ),
        )
        .child(
            col()
                .child(label_chip("Purple palette · compact square cells", &theme))
                .child(
                    demo_surface(&theme).child(
                        ContributionCalendar::new()
                            .date_range(contribution_start, contribution_end)
                            .contributions(contribution_days)
                            .palette(ContributionPalette::Purple)
                            .size(ContributionCalendarSize::Sm)
                            .shape(ContributionCellShape::Square)
                            .show_weekday_labels(false),
                    ),
                ),
        );

        let charts_treemap = section(
            "Treemap",
            "Proportional areas for hierarchical data",
            &theme,
        )
        .child(
            col()
                .child(label_chip("Repository footprint", &theme))
                .child(demo_surface(&theme).child(TreeMap::new().data(vec![
                    TreeMapNode::new("Core", 42.0),
                    TreeMapNode::new("UI", 31.0),
                    TreeMapNode::new("Examples", 15.0),
                    TreeMapNode::new("Docs", 8.0),
                    TreeMapNode::new("Tests", 12.0),
                ]))),
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
                        .child(CircularProgress::indeterminate().animation_cycles(2)),
                )
                .child(
                    col().child(label_chip("arc", &theme)).child(
                        CircularProgress::indeterminate()
                            .spinner_type(SpinnerType::Arc)
                            .animation_cycles(2)
                            .variant(ProgressVariant::Accent),
                    ),
                )
                .child(
                    col().child(label_chip("arc no track", &theme)).child(
                        CircularProgress::indeterminate()
                            .spinner_type(SpinnerType::ArcNoTrack)
                            .animation_cycles(2)
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
                .child(
                    AnimatedProgress::new("fb-ap-1")
                        .value(0.4)
                        .accessibility_label("Default progress"),
                )
                .child(label_chip("70% success + shimmer", &theme))
                .child(
                    AnimatedProgress::new("fb-ap-2")
                        .value(0.7)
                        .accessibility_label("Success progress")
                        .variant(ProgressVariant::Success)
                        .shimmer(true)
                        .shimmer_cycles(2),
                )
                .child(label_chip("90% warning, large", &theme))
                .child(
                    AnimatedProgress::new("fb-ap-3")
                        .value(0.9)
                        .accessibility_label("Warning progress")
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

        let show_fb_notifications = self.show_fb_notifications;
        let fb_notification_view = view.clone();
        let fb_indicators = section(
            "Indicators",
            "Pulse dots and notification bell with unread badge",
            &theme,
        )
        .child(
            row()
                .gap(px(12.0))
                .items_stretch()
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .min_w(px(140.0))
                        .items_center()
                        .gap(px(12.0))
                        .child(label_chip("Available", &theme))
                        .child(
                            PulseIndicator::new("fb-pulse-1")
                                .accessibility_label("Service available")
                                .animation_cycles(2),
                        ),
                )
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .min_w(px(140.0))
                        .items_center()
                        .gap(px(12.0))
                        .child(label_chip("Syncing", &theme))
                        .child(
                            PulseIndicator::new("fb-pulse-2")
                                .accessibility_label("Synchronization active")
                                .color(theme.tokens.primary)
                                .animation_cycles(2),
                        ),
                )
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .min_w(px(140.0))
                        .items_center()
                        .gap(px(12.0))
                        .child(label_chip("Urgent", &theme))
                        .child(
                            PulseIndicator::new("fb-pulse-3")
                                .accessibility_label("Urgent alert")
                                .color(theme.tokens.destructive)
                                .size(px(12.0))
                                .speed(std::time::Duration::from_secs(1))
                                .animation_cycles(2),
                        ),
                )
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .min_w(px(140.0))
                        .items_center()
                        .gap(px(8.0))
                        .child(label_chip("Inbox", &theme))
                        .child(
                            NotificationBell::new(self.fb_notifications.clone())
                                .id("fb-bell")
                                .on_click(move |_, _, cx| {
                                    fb_notification_view.update(cx, |this, cx| {
                                        this.show_fb_notifications = !this.show_fb_notifications;
                                        cx.notify();
                                    });
                                }),
                        ),
                ),
        )
        .when(show_fb_notifications, |section| {
            section.child(
                div().flex().justify_end().child(
                    NotificationCenter::new(self.fb_notifications.clone())
                        .id("fb-notification-center")
                        .max_visible(3),
                ),
            )
        });

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
                        .animation_cycles(2)
                        .w(px(280.0))
                        .h(px(72.0))
                        .rounded(theme.tokens.radius_lg)
                        .bg(theme.tokens.muted),
                )
                .child(label_chip("skeleton loader (4 lines)", &theme))
                .child(
                    div().w(px(360.0)).child(
                        SkeletonLoader::new("fb-skeleton", self.fb_skeleton.clone())
                            .accessibility_label("Loading activity feed")
                            .lines(4)
                            .line_height(px(14.0))
                            .shimmer_cycles(2),
                    ),
                ),
        );

        // ===== Navigation =====
        let menu_action = |message: &'static str| {
            let view = view.clone();
            move |window: &mut Window, cx: &mut App| {
                view.update(cx, |this, cx| {
                    this.toast_n += 1;
                    let id = this.toast_n;
                    this.toasts.update(cx, |manager, cx| {
                        manager.add_toast(
                            ToastItem::new(id, message).variant(ToastVariant::Default),
                            window,
                            cx,
                        );
                    });
                });
            }
        };
        let breadcrumb_view = view.clone();
        let nav_group_overview = view.clone();
        let nav_group_activity = view.clone();
        let nav_group_settings = view.clone();
        let nav_hierarchy_select = view.clone();
        let nav_hierarchy_toggle = view.clone();
        let virtual_item_sizes = Rc::new(
            (0..1_000)
                .map(|index| size(px(0.0), if index % 7 == 0 { px(44.0) } else { px(36.0) }))
                .collect::<Vec<_>>(),
        );
        let nav_foundations = section(
            "Navigation foundations",
            "Breadcrumb hierarchy, grouped destinations and a virtualized long list",
            &theme,
        )
        .child(
            col()
                .child(label_chip("Breadcrumbs", &theme))
                .child(
                    Breadcrumbs::new(cx)
                        .label("Component location")
                        .items(vec![
                            BreadcrumbItem::new("workspace", "Workspace").icon("home"),
                            BreadcrumbItem::new("library", "Component library"),
                            BreadcrumbItem::new("navigation", "Navigation"),
                            BreadcrumbItem::new("component", "Breadcrumbs").is_current(true),
                        ])
                        .on_click(move |id, _, cx| {
                            breadcrumb_view.update(cx, |this, cx| {
                                this.nav_breadcrumb_selected = (*id).into();
                                cx.notify();
                            });
                        }),
                )
                .child(
                    Breadcrumbs::new(cx)
                        .variant(BreadcrumbsVariant::Supporting)
                        .separator("›")
                        .label("Supporting breadcrumb")
                        .items(vec![
                            BreadcrumbItem::new("docs", "Docs"),
                            BreadcrumbItem::new("patterns", "Patterns"),
                            BreadcrumbItem::new("current", "Current page").is_current(true),
                        ]),
                )
                .child(caption(format!(
                    "Last breadcrumb action: {}",
                    self.nav_breadcrumb_selected
                ))),
        )
        .child(
            col().child(label_chip("NavMenu groups", &theme)).child(
                div().w(px(300.0)).child(
                    NavMenu::new()
                        .label("Workspace")
                        .child(
                            NavItem::new("Overview")
                                .id("nav-group-overview")
                                .icon("layout-dashboard")
                                .selected(self.nav_row_selected.as_ref() == "overview")
                                .on_click(move |_, cx| {
                                    nav_group_overview.update(cx, |this, cx| {
                                        this.nav_row_selected = "overview".into();
                                        cx.notify();
                                    });
                                }),
                        )
                        .child(
                            NavItem::new("Activity")
                                .id("nav-group-activity")
                                .icon("activity")
                                .badge("8")
                                .selected(self.nav_row_selected.as_ref() == "activity")
                                .on_click(move |_, cx| {
                                    nav_group_activity.update(cx, |this, cx| {
                                        this.nav_row_selected = "activity".into();
                                        cx.notify();
                                    });
                                }),
                        )
                        .child(
                            NavItem::new("Settings")
                                .id("nav-group-settings")
                                .icon("settings")
                                .selected(self.nav_row_selected.as_ref() == "settings")
                                .on_click(move |_, cx| {
                                    nav_group_settings.update(cx, |this, cx| {
                                        this.nav_row_selected = "settings".into();
                                        cx.notify();
                                    });
                                }),
                        ),
                ),
            ),
        )
        .child(
            col()
                .child(label_chip(
                    "NavigationMenu · hierarchical destinations",
                    &theme,
                ))
                .child(
                    demo_surface(&theme).child(
                        NavigationMenu::<SharedString>::new()
                            .orientation(NavigationMenuOrientation::Vertical)
                            .items(vec![
                                NavigationMenuItem::new("overview".into(), "Overview")
                                    .with_icon("layout-dashboard"),
                                NavigationMenuItem::new("workspace".into(), "Workspace")
                                    .with_icon("folder")
                                    .with_children(vec![
                                        NavigationMenuItem::new("components".into(), "Components"),
                                        NavigationMenuItem::new("tokens".into(), "Design tokens"),
                                        NavigationMenuItem::new("releases".into(), "Releases"),
                                    ]),
                                NavigationMenuItem::new("archive".into(), "Archive")
                                    .with_icon("archive")
                                    .disabled(true),
                            ])
                            .selected_id(self.nav_hierarchy_selected.clone())
                            .expanded_ids(self.nav_hierarchy_expanded.clone())
                            .on_select(move |id, _, cx| {
                                nav_hierarchy_select.update(cx, |this, cx| {
                                    this.nav_hierarchy_selected = id.clone();
                                    cx.notify();
                                });
                            })
                            .on_toggle(move |id, expanded, _, cx| {
                                nav_hierarchy_toggle.update(cx, |this, cx| {
                                    this.nav_hierarchy_expanded.retain(|item| item != id);
                                    if expanded {
                                        this.nav_hierarchy_expanded.push(id.clone());
                                    }
                                    cx.notify();
                                });
                            }),
                    ),
                ),
        )
        .child(
            col()
                .child(label_chip(
                    "VirtualList · 1,000 variable-height rows",
                    &theme,
                ))
                .child(
                    div()
                        .w_full()
                        .h(px(240.0))
                        .overflow_hidden()
                        .rounded(theme.tokens.radius_lg)
                        .border_1()
                        .border_color(theme.tokens.border)
                        .child(
                            v_virtual_list(
                                view.clone(),
                                "nav-virtual-list",
                                virtual_item_sizes,
                                |_this, range, _window, cx| {
                                    let theme = Theme::of(cx);
                                    range
                                        .map(|index| {
                                            div()
                                                .h(if index % 7 == 0 { px(44.0) } else { px(36.0) })
                                                .w_full()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .px(px(12.0))
                                                .border_b_1()
                                                .border_color(theme.tokens.border)
                                                .when(index % 7 == 0, |this| {
                                                    this.bg(theme.tokens.muted.opacity(0.22))
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                })
                                                .child(format!("Destination {:04}", index + 1))
                                                .child(caption(if index % 7 == 0 {
                                                    "section"
                                                } else {
                                                    "item"
                                                }))
                                        })
                                        .collect::<Vec<_>>()
                                },
                            )
                            .size_full(),
                        ),
                ),
        );
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
                            .with_shortcut("\u{2318}X")
                            .on_click(menu_action("Cut selected content")),
                        MenuItem::new("copy", "Copy")
                            .with_icon("copy")
                            .with_shortcut("\u{2318}C")
                            .on_click(menu_action("Copied to clipboard")),
                        MenuItem::new("paste", "Paste")
                            .with_icon("clipboard")
                            .with_shortcut("\u{2318}V")
                            .on_click(menu_action("Pasted from clipboard")),
                        MenuItem::separator(),
                        MenuItem::checkbox("wrap", "Word Wrap", true)
                            .on_click(menu_action("Word wrap toggled")),
                        MenuItem::submenu("share", "Share")
                            .with_icon("share-2")
                            .with_children(vec![
                                MenuItem::new("share-link", "Copy link")
                                    .with_icon("link")
                                    .on_click(menu_action("Share link copied")),
                                MenuItem::new("share-email", "Send by email")
                                    .with_icon("mail")
                                    .on_click(menu_action("Email share selected")),
                            ]),
                        MenuItem::new("delete", "Delete")
                            .with_icon("trash-2")
                            .disabled(true),
                    ])),
            );

        let nav_bold_view = view.clone();
        let nav_italic_view = view.clone();
        let nav_dashboard_view = view.clone();
        let nav_inbox_view = view.clone();
        let nav_settings_view = view.clone();
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
                        .button(
                            ToolbarButton::new("undo", "undo")
                                .tooltip("Undo")
                                .on_click(menu_action("Undo selected")),
                        )
                        .button(
                            ToolbarButton::new("redo", "redo")
                                .tooltip("Redo")
                                .on_click(menu_action("Redo selected")),
                        ),
                )
                .group(
                    ToolbarGroup::new()
                        .button(
                            ToolbarButton::new("bold", "bold")
                                .tooltip("Bold")
                                .variant(ToolbarButtonVariant::Toggle)
                                .pressed(self.nav_toolbar_bold)
                                .on_click(move |_, cx| {
                                    nav_bold_view.update(cx, |this, cx| {
                                        this.nav_toolbar_bold = !this.nav_toolbar_bold;
                                        cx.notify();
                                    });
                                }),
                        )
                        .button(
                            ToolbarButton::new("italic", "italic")
                                .tooltip("Italic")
                                .variant(ToolbarButtonVariant::Toggle)
                                .pressed(self.nav_toolbar_italic)
                                .on_click(move |_, cx| {
                                    nav_italic_view.update(cx, |this, cx| {
                                        this.nav_toolbar_italic = !this.nav_toolbar_italic;
                                        cx.notify();
                                    });
                                }),
                        )
                        .button(
                            ToolbarButton::new("font", "type")
                                .tooltip("Font")
                                .variant(ToolbarButtonVariant::Dropdown)
                                .on_click(menu_action("Font menu requested")),
                        )
                        .button(
                            ToolbarButton::new("link", "link")
                                .tooltip("Insert link")
                                .disabled(true),
                        ),
                ),
        )
        .child(label_chip("NavItem", &theme))
        .child(
            col()
                .child(
                    NavItem::new("Dashboard")
                        .id("nav-row-dashboard")
                        .icon("layout-dashboard")
                        .selected(self.nav_row_selected.as_ref() == "dashboard")
                        .on_click(move |_, cx| {
                            nav_dashboard_view.update(cx, |this, cx| {
                                this.nav_row_selected = "dashboard".into();
                                cx.notify();
                            });
                        }),
                )
                .child(
                    NavItem::new("Inbox")
                        .id("nav-row-inbox")
                        .icon("inbox")
                        .badge("12")
                        .selected(self.nav_row_selected.as_ref() == "inbox")
                        .on_click(move |_, cx| {
                            nav_inbox_view.update(cx, |this, cx| {
                                this.nav_row_selected = "inbox".into();
                                cx.notify();
                            });
                        }),
                )
                .child(
                    NavItem::new("Settings")
                        .id("nav-row-settings")
                        .icon("settings")
                        .selected(self.nav_row_selected.as_ref() == "settings")
                        .on_click(move |_, cx| {
                            nav_settings_view.update(cx, |this, cx| {
                                this.nav_row_selected = "settings".into();
                                cx.notify();
                            });
                        }),
                )
                .child(NavItem::new("Archive").icon("archive").disabled(true)),
        );

        let nav_tree_select = view.clone();
        let nav_tree_toggle = view.clone();
        let nav_tree_open = view.clone();
        let nav_side_select = view.clone();
        let nav_side_toggle = view.clone();
        let nav_top_home = view.clone();
        let nav_top_projects = view.clone();
        let nav_top_reports = view.clone();
        let nav_top_new = view.clone();
        let nav_chrome = section(
            "App chrome",
            "A composed desktop shell with top, side, file and status navigation",
            &theme,
        )
        .child(
            div()
                .w_full()
                .h(px(430.0))
                .flex()
                .flex_col()
                .overflow_hidden()
                .rounded(theme.tokens.radius_lg)
                .border_1()
                .border_color(theme.tokens.border)
                .bg(theme.tokens.background)
                .child(
                    div().border_b_1().border_color(theme.tokens.border).child(
                        TopNav::new()
                            .brand("Kael")
                            .leading_icon("sparkles")
                            .item(
                                NavItem::new("Home")
                                    .id("nav-top-home")
                                    .icon("home")
                                    .selected(self.nav_top_selected.as_ref() == "home")
                                    .on_click(move |_, cx| {
                                        nav_top_home.update(cx, |this, cx| {
                                            this.nav_top_selected = "home".into();
                                            cx.notify();
                                        });
                                    }),
                            )
                            .item(
                                NavItem::new("Projects")
                                    .id("nav-top-projects")
                                    .icon("folder")
                                    .selected(self.nav_top_selected.as_ref() == "projects")
                                    .on_click(move |_, cx| {
                                        nav_top_projects.update(cx, |this, cx| {
                                            this.nav_top_selected = "projects".into();
                                            cx.notify();
                                        });
                                    }),
                            )
                            .item(
                                NavItem::new("Reports")
                                    .id("nav-top-reports")
                                    .icon("bar-chart-3")
                                    .selected(self.nav_top_selected.as_ref() == "reports")
                                    .on_click(move |_, cx| {
                                        nav_top_reports.update(cx, |this, cx| {
                                            this.nav_top_selected = "reports".into();
                                            cx.notify();
                                        });
                                    }),
                            )
                            .trailing(
                                Button::new("nav-top-new", "New")
                                    .size(ButtonSize::Sm)
                                    .on_click(move |_, window, cx| {
                                        nav_top_new.update(cx, |this, cx| {
                                            this.toast_n += 1;
                                            let id = this.toast_n;
                                            this.toasts.update(cx, |manager, cx| {
                                                manager.add_toast(
                                                    ToastItem::new(id, "Create action selected")
                                                        .variant(ToastVariant::Default),
                                                    window,
                                                    cx,
                                                );
                                            });
                                        });
                                    }),
                            ),
                    ),
                )
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_h(px(0.0))
                        .child(
                            div()
                                .h_full()
                                .border_r_1()
                                .border_color(theme.tokens.border)
                                .child(
                                    SideNav::new()
                                        .id("astryx-side-nav")
                                        .items(vec![
                                            SideNavItem::new("overview".into(), "Overview")
                                                .with_icon("layout-dashboard"),
                                            SideNavItem::new("members".into(), "Members")
                                                .with_icon("users")
                                                .with_badge("4"),
                                            SideNavItem::new("billing".into(), "Billing")
                                                .with_icon("credit-card"),
                                        ])
                                        .selected_id(self.nav_side_selected.clone())
                                        .collapsed(self.nav_side_collapsed)
                                        .on_select(move |id, _, cx| {
                                            nav_side_select.update(cx, |this, cx| {
                                                this.nav_side_selected = id.clone();
                                                cx.notify();
                                            });
                                        })
                                        .on_toggle(move |collapsed, _, cx| {
                                            nav_side_toggle.update(cx, |this, cx| {
                                                this.nav_side_collapsed = collapsed;
                                                cx.notify();
                                            });
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .w(px(230.0))
                                .h_full()
                                .flex_shrink_0()
                                .border_r_1()
                                .border_color(theme.tokens.border)
                                .p(px(12.0))
                                .child(label_chip("Files", &theme))
                                .child(
                                    FileTree::new()
                                        .id("astryx-file-tree")
                                        .nodes(vec![
                                            FileNode::directory("src").with_children(vec![
                                                FileNode::file("main.rs"),
                                                FileNode::file("lib.rs"),
                                                FileNode::directory("ui").with_children(vec![
                                                    FileNode::file("button.rs"),
                                                ]),
                                            ]),
                                            FileNode::file("Cargo.toml"),
                                            FileNode::file("README.md"),
                                        ])
                                        .expanded_paths(self.nav_expanded_paths.clone())
                                        .selected_path(self.nav_selected_path.clone())
                                        .show_file_size(false)
                                        .on_select(move |path, _, cx| {
                                            nav_tree_select.update(cx, |this, cx| {
                                                this.nav_selected_path = path.clone();
                                                cx.notify();
                                            });
                                        })
                                        .on_toggle(move |path, expanded, _, cx| {
                                            nav_tree_toggle.update(cx, |this, cx| {
                                                if expanded {
                                                    if !this.nav_expanded_paths.contains(path) {
                                                        this.nav_expanded_paths.push(path.clone());
                                                    }
                                                } else {
                                                    this.nav_expanded_paths
                                                        .retain(|candidate| candidate != path);
                                                }
                                                cx.notify();
                                            });
                                        })
                                        .on_open(move |path, window, cx| {
                                            nav_tree_open.update(cx, |this, cx| {
                                                this.toast_n += 1;
                                                let id = this.toast_n;
                                                let title = format!("Opened {}", path.display());
                                                this.toasts.update(cx, |manager, cx| {
                                                    manager.add_toast(
                                                        ToastItem::new(id, title)
                                                            .variant(ToastVariant::Default),
                                                        window,
                                                        cx,
                                                    );
                                                });
                                            });
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .h_full()
                                .p(px(20.0))
                                .bg(theme.tokens.muted.opacity(0.18))
                                .child(
                                    div()
                                        .h_full()
                                        .rounded(theme.tokens.radius_lg)
                                        .border_1()
                                        .border_color(theme.tokens.border)
                                        .bg(theme.tokens.card)
                                        .p(px(20.0))
                                        .child(h3("main.rs".to_string()))
                                        .child(
                                            muted(
                                                "Choose a file to update the workspace preview."
                                                    .to_string(),
                                            )
                                            .mt(px(6.0)),
                                        ),
                                ),
                        ),
                )
                .child(
                    div()
                        .border_t_1()
                        .border_color(theme.tokens.border)
                        .child(self.nav_status_bar.clone()),
                ),
        );

        let nav_mobile_toggle = view.clone();
        let nav_mobile_overview = view.clone();
        let nav_mobile_activity = view.clone();
        let nav_mobile = section(
            "Compact navigation",
            "Responsive navigation header with an independently controlled menu",
            &theme,
        )
        .child(
            div()
                .w(px(380.0))
                .overflow_hidden()
                .rounded(theme.tokens.radius_lg)
                .border_1()
                .border_color(theme.tokens.border)
                .child(
                    MobileNav::new("Project workspace")
                        .open(self.nav_mobile_open)
                        .on_toggle(move |open, _, cx| {
                            nav_mobile_toggle.update(cx, |this, cx| {
                                this.nav_mobile_open = open;
                                cx.notify();
                            });
                        })
                        .item(
                            NavItem::new("Overview")
                                .id("mobile-nav-overview")
                                .icon("layout-dashboard")
                                .selected(self.nav_row_selected.as_ref() == "mobile-overview")
                                .on_click(move |_, cx| {
                                    nav_mobile_overview.update(cx, |this, cx| {
                                        this.nav_row_selected = "mobile-overview".into();
                                        cx.notify();
                                    });
                                }),
                        )
                        .item(
                            NavItem::new("Activity")
                                .id("mobile-nav-activity")
                                .icon("activity")
                                .badge("8")
                                .selected(self.nav_row_selected.as_ref() == "mobile-activity")
                                .on_click(move |_, cx| {
                                    nav_mobile_activity.update(cx, |this, cx| {
                                        this.nav_row_selected = "mobile-activity".into();
                                        cx.notify();
                                    });
                                }),
                        ),
                ),
        );

        // ===== Overlays =====
        let overlays_alert = section(
            "Alert dialog",
            "Confirmation prompt with cancel and action",
            &theme,
        )
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(16.0))
                .p(px(20.0))
                .rounded(theme.tokens.radius_lg)
                .bg(theme.tokens.muted.opacity(0.25))
                .child(muted(
                    "Destructive confirmations open only when requested.".to_string(),
                ))
                .child(
                    Button::new("open-alert-dialog", "Open alert dialog")
                        .variant(ButtonVariant::Destructive)
                        .on_click({
                            let view = view.clone();
                            move |_, _, cx| {
                                view.update(cx, |this, cx| {
                                    this.show_alert_dialog = true;
                                    cx.notify();
                                });
                            }
                        }),
                ),
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
                .h(px(220.0))
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
                row().items_start().flex_wrap().gap(px(12.0)).children([
                    col()
                        .w(px(196.0))
                        .h(px(124.0))
                        .p(px(12.0))
                        .gap(px(14.0))
                        .rounded(theme.tokens.radius_lg)
                        .border_1()
                        .border_color(theme.tokens.border)
                        .bg(theme.tokens.muted.opacity(0.18))
                        .child(label_chip("top", &theme))
                        .child(
                            div().flex_1().flex().items_center().justify_center().child(
                                Tooltip::new("Tooltip on top")
                                    .placement(TooltipPlacement::Top)
                                    .default_open(true)
                                    .child(
                                        Button::new("ovl-tip-top", "Hover")
                                            .variant(ButtonVariant::Outline),
                                    ),
                            ),
                        ),
                    col()
                        .w(px(196.0))
                        .h(px(124.0))
                        .p(px(12.0))
                        .gap(px(14.0))
                        .rounded(theme.tokens.radius_lg)
                        .border_1()
                        .border_color(theme.tokens.border)
                        .bg(theme.tokens.muted.opacity(0.18))
                        .child(label_chip("bottom", &theme))
                        .child(
                            div().flex_1().flex().items_center().justify_center().child(
                                Tooltip::new("Tooltip on bottom")
                                    .placement(TooltipPlacement::Bottom)
                                    .default_open(true)
                                    .child(
                                        Button::new("ovl-tip-bottom", "Hover")
                                            .variant(ButtonVariant::Outline),
                                    ),
                            ),
                        ),
                    col()
                        .w(px(196.0))
                        .h(px(124.0))
                        .p(px(12.0))
                        .gap(px(14.0))
                        .rounded(theme.tokens.radius_lg)
                        .border_1()
                        .border_color(theme.tokens.border)
                        .bg(theme.tokens.muted.opacity(0.18))
                        .child(label_chip("end aligned", &theme))
                        .child(
                            div().flex_1().flex().items_center().justify_center().child(
                                Tooltip::new("Aligned to the end")
                                    .placement(TooltipPlacement::Top)
                                    .alignment(TooltipAlignment::End)
                                    .default_open(true)
                                    .child(
                                        Button::new("ovl-tip-end", "Hover")
                                            .variant(ButtonVariant::Outline),
                                    ),
                            ),
                        ),
                    col()
                        .w(px(196.0))
                        .h(px(124.0))
                        .p(px(12.0))
                        .gap(px(14.0))
                        .rounded(theme.tokens.radius_lg)
                        .border_1()
                        .border_color(theme.tokens.border)
                        .bg(theme.tokens.muted.opacity(0.18))
                        .child(label_chip("hover only", &theme))
                        .child(
                            div().flex_1().flex().items_center().justify_center().child(
                                Tooltip::new("Appears on hover")
                                    .placement(TooltipPlacement::Bottom)
                                    .child(
                                        Button::new("ovl-tip-hover", "Hover me")
                                            .variant(ButtonVariant::Secondary),
                                    ),
                            ),
                        ),
                ]),
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
                .flex_wrap()
                .gap(px(14.0))
                .child(
                    Link::new("Inline link")
                        .variant(LinkVariant::Inline)
                        .href("https://augani.github.io/kael/"),
                )
                .child(
                    Link::new("Underlined")
                        .variant(LinkVariant::Inline)
                        .underline(true)
                        .href("https://augani.github.io/kael/"),
                )
                .child(
                    Link::new("Subtle link")
                        .variant(LinkVariant::Subtle)
                        .href("https://github.com/Augani/kael"),
                )
                .child(
                    Link::new("External link")
                        .external(true)
                        .href("https://github.com/Augani/kael"),
                )
                .child(
                    Link::new("Disabled link")
                        .variant(LinkVariant::Inline)
                        .href("https://augani.github.io/kael/")
                        .disabled(true),
                ),
        )
        .child(
            col().child(label_chip("Block", &theme)).child(
                Link::new("Block-style link row")
                    .variant(LinkVariant::Block)
                    .href("https://augani.github.io/kael/"),
            ),
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
            "Motion Typography",
            "Marquee, typewriter, and staggered per-character entrance",
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
                .accessibility_label("Native Rust GPUI · Astryx design system · 60fps typography")
                .speed(60.0)
                .direction(MarqueeDirection::Left)
                .pause_on_hover(true)
                .paused(std::env::var_os("ASTRYX_SHOWCASE_PAUSE_MOTION").is_some())
                .content_width(px(420.0))
                .w_full(),
            ),
        )
        .child(
            col()
                .child(label_chip("TypeWriter", &theme))
                .child(
                    TypeWriter::new("typography-typewriter", self.typography_typewriter.clone())
                        .text_size(px(20.0))
                        .font_weight(FontWeight::MEDIUM),
                )
                .child(
                    Button::new("typography-typewriter-replay", "Replay")
                        .variant(ButtonVariant::Outline)
                        .size(ButtonSize::Sm)
                        .on_click({
                            let state = self.typography_typewriter.clone();
                            move |_, _, cx| {
                                state.update(cx, |state, cx| state.start(cx));
                            }
                        }),
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
        )
        .child(
            row()
                .items_stretch()
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .min_w(px(0.0))
                        .child(label_chip("TextHighlight", &theme))
                        .child(
                            TextHighlight::new("typography-highlight")
                                .color(theme.tokens.warning.opacity(0.32))
                                .duration(std::time::Duration::from_millis(520))
                                .px(px(4.0))
                                .py(px(2.0))
                                .child(
                                    Text::new("Important details stay readable during motion")
                                        .size(px(18.0))
                                        .weight(FontWeight::SEMIBOLD),
                                ),
                        ),
                )
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .min_w(px(0.0))
                        .child(label_chip("TextReveal", &theme))
                        .child(
                            TextReveal::new(
                                "typography-reveal",
                                "Accessible motion should support comprehension.",
                            )
                            .mode(RevealMode::ByWord)
                            .stagger(std::time::Duration::from_millis(45))
                            .text_size(px(19.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.tokens.foreground),
                        ),
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
                    col().child(label_chip("Image", &theme)).child(
                        Thumbnail::new()
                            .src("assets/images/carousel_1.jpg")
                            .alt("A pale antler draped with crimson fabric"),
                    ),
                )
                .child(col().child(label_chip("Loading", &theme)).child(
                    Thumbnail::new().loading(true).loading_animation(
                        std::env::var_os("ASTRYX_SHOWCASE_PAUSE_MOTION").is_none(),
                    ),
                ))
                .child(
                    col().child(label_chip("Disabled", &theme)).child(
                        Thumbnail::new()
                            .src("assets/images/carousel_2.jpg")
                            .alt("Golden sunlight over misty forested hills")
                            .disabled(true),
                    ),
                )
                .child(
                    col().child(label_chip("With label", &theme)).child(
                        Thumbnail::new()
                            .src("assets/images/carousel_3.jpg")
                            .alt("A translucent spider web across moss")
                            .label("cover.png"),
                    ),
                ),
        );

        let media_waveform = section(
            "Waveform",
            "Responsive audio amplitude with playback progress",
            &theme,
        )
        .child(
            demo_surface(&theme)
                .w_full()
                .child(label_chip("42% playback", &theme))
                .child(
                    Waveform::new()
                        .data(&[
                            0.18, 0.42, 0.76, 0.34, 0.58, 0.92, 0.64, 0.28, 0.48, 0.82, 0.55, 0.22,
                            0.7, 0.96, 0.6, 0.38, 0.74, 0.5, 0.84, 0.32,
                        ])
                        .playback_position(0.42)
                        .h(px(72.0)),
                ),
        );

        let media_players = section(
            "Media players",
            "Keyboard-operable audio and video transport controls",
            &theme,
        )
        .child(
            col()
                .gap(px(8.0))
                .child(label_chip("AudioPlayer — full controls", &theme))
                .child(
                    AudioPlayer::new(self.media_audio.clone())
                        .id("showcase-audio-player")
                        .title("Design systems field notes"),
                ),
        )
        .child(
            col()
                .gap(px(8.0))
                .child(label_chip("VideoPlayer — poster and controls", &theme))
                .child(
                    VideoPlayer::new(self.media_video.clone())
                        .id("showcase-video-player")
                        .size(VideoPlayerSize::Sm),
                ),
        );

        let media_viewer = section(
            "Image Viewer",
            "Keyboard navigation, zoom, captions, and thumbnails",
            &theme,
        )
        .child(
            demo_surface(&theme)
                .flex()
                .items_center()
                .justify_between()
                .gap(px(16.0))
                .child(
                    col()
                        .gap(px(4.0))
                        .child(body("Open an accessible gallery overlay"))
                        .child(caption(
                            "Use arrow keys to browse, plus or minus to zoom, and Escape to close.",
                        )),
                )
                .child(
                    Button::new("media-open-viewer", "Open gallery")
                        .icon("image")
                        .on_click({
                            let view = view.clone();
                            move |_, _, cx| {
                                view.update(cx, |this, cx| {
                                    this.show_media_viewer = true;
                                    cx.notify();
                                });
                            }
                        }),
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

        let confetti_state = self.effects_confetti.clone();
        let particle_state = self.effects_particles.clone();
        let particles_running = particle_state.read(cx).is_running();
        let ambient_motion_paused = self.effects_meteors.read(cx).is_paused();
        let ambient_motion_state = self.effects_meteors.clone();
        let aurora_motion_state = self.effects_aurora.clone();
        let reduce_motion = cx.reduce_motion();
        let media_effects = section(
            "Ambient & Particle Effects",
            "Decorative motion with bounded canvases and reduced-motion support",
            &theme,
        )
        .child(
            row()
                .items_center()
                .justify_between()
                .child(muted(if reduce_motion {
                    "Motion is reduced by your system accessibility setting."
                } else if ambient_motion_paused {
                    "Ambient previews are paused to conserve power."
                } else {
                    "Ambient previews are playing."
                }))
                .child(
                    Button::new(
                        "effects-ambient-toggle",
                        if ambient_motion_paused {
                            "Play ambient motion"
                        } else {
                            "Pause ambient motion"
                        },
                    )
                    .variant(ButtonVariant::Secondary)
                    .size(ButtonSize::Sm)
                    .disabled(reduce_motion)
                    .on_click(move |_, _, cx| {
                        ambient_motion_state.update(cx, |state, cx| {
                            if state.is_paused() {
                                state.resume(cx);
                            } else {
                                state.pause(cx);
                            }
                        });
                        aurora_motion_state.update(cx, |state, cx| {
                            if state.is_paused() {
                                state.resume(cx);
                            } else {
                                state.pause(cx);
                            }
                        });
                    }),
                ),
        )
        .child(
            row()
                .items_stretch()
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .min_w(px(0.0))
                        .child(label_chip("Aurora", &theme))
                        .child(
                            Aurora::new()
                                .state(self.effects_aurora.clone())
                                .colors(vec![
                                    hsla(0.61, 0.82, 0.58, 0.34),
                                    hsla(0.78, 0.72, 0.62, 0.28),
                                    hsla(0.48, 0.72, 0.52, 0.24),
                                ])
                                .speed(0.72)
                                .animated(!ambient_motion_paused)
                                .h(px(210.0))
                                .w_full()
                                .rounded(theme.tokens.radius_lg)
                                .overflow_hidden()
                                .child(
                                    col()
                                        .size_full()
                                        .items_center()
                                        .justify_center()
                                        .gap(px(4.0))
                                        .child(
                                            Heading::new("Calm by default").level(HeadingLevel::H4),
                                        )
                                        .child(caption("Organic color, quiet movement")),
                                ),
                        ),
                )
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .min_w(px(0.0))
                        .child(label_chip("Meteors", &theme))
                        .child(
                            Meteors::new("showcase-meteors", self.effects_meteors.clone())
                                .count(10)
                                .speed(0.8)
                                .angle(222.0)
                                .trail_length(px(96.0))
                                .color(hsla(0.56, 0.9, 0.72, 0.72))
                                .h(px(210.0))
                                .w_full()
                                .rounded(theme.tokens.radius_lg)
                                .bg(hsla(0.63, 0.34, 0.11, 1.0))
                                .child(
                                    col()
                                        .size_full()
                                        .items_center()
                                        .justify_center()
                                        .gap(px(4.0))
                                        .text_color(white())
                                        .child(
                                            Heading::new("Nightly build")
                                                .level(HeadingLevel::H4)
                                                .color(white()),
                                        )
                                        .child(
                                            caption("10 checks moving through the pipeline")
                                                .text_color(TextColor::Inherit),
                                        ),
                                ),
                        ),
                ),
        )
        .child(
            row()
                .items_stretch()
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .min_w(px(0.0))
                        .child(label_chip("Confetti", &theme))
                        .child(
                            div()
                                .relative()
                                .h(px(220.0))
                                .w_full()
                                .overflow_hidden()
                                .rounded(theme.tokens.radius_lg)
                                .border_1()
                                .border_color(theme.tokens.border)
                                .bg(theme.tokens.muted)
                                .child(
                                    Confetti::new(
                                        "showcase-confetti",
                                        self.effects_confetti.clone(),
                                    )
                                    .particle_count(96, cx)
                                    .gravity(110.0, cx)
                                    .spread(270.0, cx)
                                    .size_full(),
                                )
                                .child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            Button::new("effects-confetti-burst", "Celebrate")
                                                .icon("sparkles")
                                                .disabled(reduce_motion)
                                                .on_click(move |_, _, cx| {
                                                    confetti_state
                                                        .update(cx, |state, cx| state.burst(cx));
                                                }),
                                        ),
                                ),
                        ),
                )
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .min_w(px(0.0))
                        .child(label_chip("Particle emitter", &theme))
                        .child(
                            div()
                                .relative()
                                .h(px(220.0))
                                .w_full()
                                .overflow_hidden()
                                .rounded(theme.tokens.radius_lg)
                                .border_1()
                                .border_color(theme.tokens.border)
                                .bg(hsla(0.62, 0.28, 0.12, 1.0))
                                .child(
                                    ParticleEmitter::new(
                                        "showcase-particle-emitter",
                                        self.effects_particles.clone(),
                                    )
                                    .size_full(),
                                )
                                .child(
                                    div().absolute().top(px(14.0)).left(px(14.0)).child(
                                        Button::new(
                                            "effects-particles-toggle",
                                            if particles_running {
                                                "Stop emitter"
                                            } else {
                                                "Start emitter"
                                            },
                                        )
                                        .size(ButtonSize::Sm)
                                        .disabled(reduce_motion)
                                        .on_click(
                                            move |_, _, cx| {
                                                particle_state.update(cx, |state, cx| {
                                                    if state.is_running() {
                                                        state.stop(cx);
                                                    } else {
                                                        state.start(cx);
                                                    }
                                                });
                                            },
                                        ),
                                    ),
                                ),
                        ),
                ),
        );

        // ===== Layout =====
        let theme = theme.clone();

        let swatch = |bg: Hsla, label_text: &str| {
            let foreground = if bg.l < 0.42 {
                hsla(0.0, 0.0, 1.0, 1.0)
            } else {
                theme.tokens.foreground
            };
            div()
                .flex()
                .items_center()
                .justify_center()
                .w_full()
                .min_w(px(56.0))
                .h(px(48.0))
                .rounded(theme.tokens.radius_md)
                .bg(bg)
                .text_color(foreground)
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .child(label_text.to_string())
        };

        let masonry_tile = |bg: Hsla, label_text: &str| {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme.tokens.radius_md)
                .bg(bg)
                .text_color(theme.tokens.foreground)
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .child(label_text.to_string())
        };

        let layout_primitives_section = section(
            "Core layout primitives",
            "Purpose-built stacks, wrapping flows, spacing and bounded content",
            &theme,
        )
        .child(
            Grid::new()
                .columns(2)
                .gap(px(12.0))
                .alignment(GridAlignment::Stretch)
                .child(
                    demo_surface(&theme)
                        .min_w(px(0.0))
                        .child(label_chip("VStack · rhythm and alignment", &theme))
                        .child(
                            VStack::new()
                                .spacing(px(8.0))
                                .align(Align::Stretch)
                                .fill_width()
                                .child(swatch(theme.tokens.muted, "First"))
                                .child(swatch(theme.tokens.accent, "Second"))
                                .child(swatch(theme.tokens.muted, "Third")),
                        ),
                )
                .child(
                    demo_surface(&theme)
                        .min_w(px(0.0))
                        .child(label_chip("HStack · flexible spacer", &theme))
                        .child(
                            HStack::new()
                                .spacing(px(8.0))
                                .align(Align::Center)
                                .fill_width()
                                .child(
                                    div()
                                        .px(px(12.0))
                                        .py(px(8.0))
                                        .rounded(theme.tokens.radius_md)
                                        .bg(theme.tokens.muted)
                                        .child("Leading"),
                                )
                                .child(Spacer::new())
                                .child(
                                    Button::new("layout-spacer-action", "Action")
                                        .size(ButtonSize::Sm),
                                ),
                        ),
                )
                .child(
                    demo_surface(&theme)
                        .min_w(px(0.0))
                        .child(label_chip("Flow · responsive wrapping", &theme))
                        .child(
                            Flow::new()
                                .spacing(px(8.0))
                                .align(Align::Center)
                                .children(["Design", "Engineering", "Research", "Operations"].map(
                                    |label| {
                                        div()
                                            .px(px(10.0))
                                            .py(px(6.0))
                                            .rounded_full()
                                            .border_1()
                                            .border_color(theme.tokens.border)
                                            .bg(theme.tokens.card)
                                            .text_size(px(12.0))
                                            .child(label)
                                    },
                                )),
                        ),
                )
                .child(
                    demo_surface(&theme)
                        .min_w(px(0.0))
                        .child(label_chip("Cluster · compact actions", &theme))
                        .child(
                            Cluster::new()
                                .spacing(px(8.0))
                                .align(Align::Center)
                                .child(Button::new("layout-cluster-save", "Save").size(ButtonSize::Sm))
                                .child(
                                    Button::new("layout-cluster-preview", "Preview")
                                        .variant(ButtonVariant::Secondary)
                                        .size(ButtonSize::Sm),
                                )
                                .child(
                                    Button::new("layout-cluster-more", "More")
                                        .variant(ButtonVariant::Ghost)
                                        .size(ButtonSize::Sm),
                                ),
                        ),
                ),
        )
        .child(
            demo_surface(&theme)
                .child(label_chip("Container + Panel · readable content bounds", &theme))
                .child(
                    Container::sm().child(
                        kael_ui::layout::Panel::new()
                            .card()
                            .w_full()
                            .bg(theme.tokens.card)
                            .border_color(theme.tokens.border)
                            .child(
                                VStack::new()
                                    .spacing(px(6.0))
                                    .child(h4("Bounded workspace surface".to_string()))
                                    .child(muted(
                                        "Container controls measure; Panel supplies the local surface and padding.",
                                    )),
                            ),
                    ),
                ),
        );

        let layout_stack_section = section(
            "Stack, Center, Section & AspectRatio",
            "Flex containers, centering, surface regions and fixed ratios",
            &theme,
        )
        .child(
            row()
                .items_stretch()
                .child(
                    demo_surface(&theme)
                        .w(px(300.0))
                        .child(label_chip("Vertical stack", &theme))
                        .child(
                            Stack::new()
                                .vertical()
                                .gap(px(8.0))
                                .align(Align::Stretch)
                                .w_full()
                                .child(swatch(theme.tokens.muted, "First"))
                                .child(swatch(theme.tokens.muted, "Second"))
                                .child(swatch(theme.tokens.muted, "Third")),
                        ),
                )
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .child(label_chip("Horizontal stack with fill", &theme))
                        .child(
                            Stack::new()
                                .horizontal()
                                .gap(px(8.0))
                                .align(Align::Stretch)
                                .w_full()
                                .child(StackItem::new(
                                    div().w(px(92.0)).child(swatch(theme.tokens.muted, "Fixed")),
                                ))
                                .child(
                                    StackItem::new(swatch(theme.tokens.accent, "Flexible")).fill(),
                                ),
                        ),
                ),
        )
        .child(
            row()
                .items_stretch()
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .child(label_chip("Center — both axes", &theme))
                        .child(
                            Center::new()
                                .axis(CenterAxis::Both)
                                .height(px(112.0))
                                .child(
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
                    demo_surface(&theme)
                        .flex_1()
                        .child(label_chip("Section variants", &theme))
                        .child(
                            col()
                                .child(
                                    Section::new()
                                        .variant(SectionVariant::Section)
                                        .padding(px(12.0))
                                        .border_1()
                                        .border_color(theme.tokens.border)
                                        .rounded(theme.tokens.radius_md)
                                        .child(body("Section surface")),
                                )
                                .child(
                                    Section::new()
                                        .variant(SectionVariant::Muted)
                                        .padding(px(12.0))
                                        .divider(SectionDivider::Start)
                                        .rounded(theme.tokens.radius_md)
                                        .child(body("Muted + start divider")),
                                ),
                        ),
                )
                .child(
                    demo_surface(&theme)
                        .w(px(300.0))
                        .child(label_chip("AspectRatio", &theme))
                        .child(
                            row()
                                .flex_nowrap()
                                .items_start()
                                .child(
                                    div().w(px(158.0)).child(AspectRatio::new(
                                        16.0 / 9.0,
                                        div()
                                            .size_full()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .bg(theme.tokens.accent)
                                            .child("16:9"),
                                    )),
                                )
                                .child(
                                    div().w(px(80.0)).child(
                                        AspectRatio::new(
                                            1.0,
                                            div()
                                                .size_full()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .bg(theme.tokens.muted)
                                                .child("1:1"),
                                        )
                                        .shape(AspectRatioShape::Ellipse),
                                    ),
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
            demo_surface(&theme)
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
            demo_surface(&theme)
                .child(label_chip("MasonryGrid — 3 columns", &theme))
                .child(
                    MasonryGrid::new()
                        .columns(3)
                        .gap(px(8.0))
                        .fill_width()
                        .item(masonry_tile(theme.tokens.muted, "tall"), 120.0)
                        .item(masonry_tile(theme.tokens.accent, "short"), 56.0)
                        .item(masonry_tile(theme.tokens.muted, "mid"), 88.0)
                        .item(masonry_tile(theme.tokens.accent, "short"), 56.0)
                        .item(masonry_tile(theme.tokens.muted, "tall"), 120.0)
                        .item(masonry_tile(theme.tokens.accent, "mid"), 88.0),
                ),
        );

        let layout_separator_section = section(
            "Separator / Divider",
            "Horizontal and vertical dividers with weights and labels",
            &theme,
        )
        .child(
            Grid::new()
                .columns(2)
                .gap(px(12.0))
                .child(
                    demo_surface(&theme)
                        .child(label_chip("Subtle", &theme))
                        .child(Separator::new()),
                )
                .child(
                    demo_surface(&theme)
                        .child(label_chip("Strong", &theme))
                        .child(Separator::new().variant(SeparatorVariant::Strong)),
                )
                .child(
                    demo_surface(&theme)
                        .child(label_chip("With centered label", &theme))
                        .child(Separator::new().label("OR")),
                )
                .child(
                    demo_surface(&theme)
                        .child(label_chip("Vertical", &theme))
                        .child(
                            row()
                                .h(px(48.0))
                                .gap(px(16.0))
                                .items_center()
                                .child(body("left"))
                                .child(
                                    Separator::vertical()
                                        .variant(SeparatorVariant::Strong)
                                        .h(px(28.0)),
                                )
                                .child(body("right")),
                        ),
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
                        .label("Toggle details")
                        .open(collapsible_open)
                        .trigger(div().w_full().p(px(12.0)).child(body("Toggle details")))
                        .content(
                            div()
                                .px(px(12.0))
                                .pb(px(12.0))
                                .child(muted("Hidden content revealed when open.")),
                        )
                        .on_toggle(move |open, _window, cx| {
                            collapsible_view.update(cx, |this, cx| {
                                this.layout_collapsible_open = open;
                                cx.notify();
                            });
                        })
                        .border_1()
                        .border_color(theme.tokens.border)
                        .rounded(theme.tokens.radius_lg)
                        .overflow_hidden(),
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
                                .label("First item")
                                .open(group_a)
                                .trigger(div().w_full().p(px(12.0)).child(body("First item")))
                                .content(
                                    div().px(px(12.0)).pb(px(12.0)).child(muted("First body.")),
                                )
                                .on_toggle(move |open, _window, cx| {
                                    view_a.update(cx, |this, cx| {
                                        this.layout_collapsible_a = open;
                                        cx.notify();
                                    });
                                }),
                        )
                        .child(
                            Collapsible::new()
                                .label("Second item")
                                .open(group_b)
                                .trigger(div().w_full().p(px(12.0)).child(body("Second item")))
                                .content(
                                    div().px(px(12.0)).pb(px(12.0)).child(muted("Second body.")),
                                )
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
                    div()
                        .h(px(160.0))
                        .w_full()
                        .border_1()
                        .border_color(theme.tokens.border)
                        .rounded(theme.tokens.radius_lg)
                        .overflow_hidden()
                        .child(
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
                    div()
                        .h(px(140.0))
                        .w_full()
                        .border_1()
                        .border_color(theme.tokens.border)
                        .rounded(theme.tokens.radius_lg)
                        .overflow_hidden()
                        .child(
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

        let animated_collapsible_view = view.clone();
        let animated_list_view = view.clone();
        let animated_list_state = self.layout_animated_list.clone();
        let next_animated_item_count = if self.layout_animated_item_count >= 5 {
            3
        } else {
            self.layout_animated_item_count + 1
        };
        let presence_view = view.clone();
        let transition_view = view.clone();
        let shared_view = view.clone();
        let shared_state = self.layout_shared.clone();
        let shared_progress = self.layout_shared.read(cx).progress();
        let ripple_view = view.clone();
        let ripple_version = self.effects_ripple_version;
        let drawer_state = self.layout_drawer.clone();
        let sortable_theme = theme.clone();
        let infinite_view = view.clone();
        let infinite_state = self.layout_infinite.clone();
        let infinite_count = self.layout_infinite_count;
        let drag_drop_view = view.clone();
        let drag_drop_state = self.layout_drag_drop.clone();
        let drag_item_grabbed = self.layout_drag_drop.read(cx).has_grabbed_item();

        let motion_item = |title: &'static str, detail: &'static str, accent: Hsla| {
            row()
                .items_center()
                .gap(px(10.0))
                .w_full()
                .p(px(10.0))
                .rounded(theme.tokens.radius_md)
                .border_1()
                .border_color(theme.tokens.border)
                .bg(theme.tokens.card)
                .child(
                    div()
                        .size(px(8.0))
                        .rounded_full()
                        .bg(accent)
                        .flex_shrink_0(),
                )
                .child(
                    col()
                        .gap(px(2.0))
                        .child(label(title.to_string()))
                        .child(caption(detail.to_string())),
                )
        };

        let layout_motion_section = section(
            "Motion & Stateful Layout",
            "Interruptible transitions, keyboard reordering and progressive content",
            &theme,
        )
        .child(
            demo_surface(&theme)
                .child(label_chip("Draggable + DropZone · pointer and keyboard", &theme))
                .child(caption(
                    "Drag the card, or focus it and press Enter; focus the drop zone and press Enter again.",
                ))
                .child(
                    row()
                        .items_stretch()
                        .child(
                            Draggable::new(
                                "layout-drag-source",
                                DragData::new(SharedString::from("Release notes"))
                                    .with_label("Release notes card"),
                            )
                            .keyboard_state(&self.layout_drag_drop)
                            .accessibility_label("Release notes card")
                            .hover_bg(theme.tokens.accent.opacity(0.25))
                            .w(px(260.0))
                            .child(
                                row()
                                    .items_center()
                                    .gap(px(10.0))
                                    .w_full()
                                    .p(px(12.0))
                                    .rounded(theme.tokens.radius_lg)
                                    .border_1()
                                    .border_color(if drag_item_grabbed {
                                        theme.tokens.primary
                                    } else {
                                        theme.tokens.border
                                    })
                                    .bg(theme.tokens.card)
                                    .child(Icon::new("grip-vertical").size(px(16.0)))
                                    .child(
                                        col()
                                            .gap(px(2.0))
                                            .child(label("Release notes".to_string()))
                                            .child(caption(if drag_item_grabbed {
                                                "Picked up — choose a destination"
                                            } else {
                                                "Ready to move"
                                            })),
                                    ),
                            ),
                        )
                        .child(
                            DropZone::<SharedString>::new("layout-drop-target")
                                .keyboard_state(&drag_drop_state)
                                .accessibility_label("Publish queue drop zone")
                                .drop_zone_style(DropZoneStyle::Dashed)
                                .min_h(px(92.0))
                                .flex_1()
                                .on_drop(move |data, _, cx| {
                                    drag_drop_view.update(cx, |this, cx| {
                                        this.layout_last_drop = data.data.clone();
                                        cx.notify();
                                    });
                                })
                                .child(
                                    col()
                                        .items_center()
                                        .gap(px(4.0))
                                        .child(Icon::new("inbox").size(px(20.0)))
                                        .child(label("Publish queue".to_string()))
                                        .child(caption(format!(
                                            "Last received: {}",
                                            self.layout_last_drop
                                        ))),
                                ),
                        ),
                ),
        )
        .child(
            row()
                .items_stretch()
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .min_w(px(0.0))
                        .child(label_chip("AnimatedCollapsible", &theme))
                        .child(
                            AnimatedCollapsible::new()
                                .id("layout-animated-collapsible")
                                .label("Release readiness details")
                                .open(self.layout_collapsible_open)
                                .trigger(
                                    row()
                                        .items_center()
                                        .justify_between()
                                        .w_full()
                                        .p(px(10.0))
                                        .child(label("Release readiness".to_string()))
                                        .child(Badge::new("3 checks").variant(BadgeVariant::Info)),
                                )
                                .content(
                                    col()
                                        .gap(px(8.0))
                                        .px(px(10.0))
                                        .pb(px(10.0))
                                        .child(muted("Keyboard navigation verified"))
                                        .child(muted("Reduced motion verified"))
                                        .child(muted("Screen-reader labels verified")),
                                )
                                .on_toggle(move |open, _, cx| {
                                    animated_collapsible_view.update(cx, |this, cx| {
                                        this.layout_collapsible_open = open;
                                        cx.notify();
                                    });
                                })
                                .border_1()
                                .border_color(theme.tokens.border)
                                .rounded(theme.tokens.radius_lg)
                                .overflow_hidden(),
                        ),
                )
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .min_w(px(0.0))
                        .child(label_chip("AnimatedPresence", &theme))
                        .child(
                            col()
                                .gap(px(10.0))
                                .child(
                                    Button::new(
                                        "layout-presence-toggle",
                                        if self.layout_presence_visible {
                                            "Hide status"
                                        } else {
                                            "Show status"
                                        },
                                    )
                                    .variant(ButtonVariant::Secondary)
                                    .size(ButtonSize::Sm)
                                    .on_click(
                                        move |_, _, cx| {
                                            presence_view.update(cx, |this, cx| {
                                                this.layout_presence_visible =
                                                    !this.layout_presence_visible;
                                                cx.notify();
                                            });
                                        },
                                    ),
                                )
                                .child(
                                    AnimatedPresence::new(
                                        "layout-presence",
                                        self.layout_presence.clone(),
                                    )
                                    .show(self.layout_presence_visible)
                                    .child(
                                        Alert::success()
                                            .title("Ready for review")
                                            .description("All automated checks passed."),
                                    ),
                                ),
                        ),
                ),
        )
        .child(
            row()
                .items_stretch()
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .min_w(px(0.0))
                        .child(
                            row()
                                .items_center()
                                .justify_between()
                                .child(label_chip("AnimatedList", &theme))
                                .child(
                                    Button::new("layout-list-change", "Change items")
                                        .variant(ButtonVariant::Ghost)
                                        .size(ButtonSize::Sm)
                                        .on_click(move |_, _, cx| {
                                            let keys = [
                                                "design", "build", "review", "document", "release",
                                            ]
                                            .into_iter()
                                            .take(next_animated_item_count)
                                            .map(SharedString::from)
                                            .collect();
                                            animated_list_state.update(cx, |state, cx| {
                                                state.set_keys(keys, cx);
                                            });
                                            animated_list_view.update(cx, |this, cx| {
                                                this.layout_animated_item_count =
                                                    next_animated_item_count;
                                                cx.notify();
                                            });
                                        }),
                                ),
                        )
                        .child(
                            AnimatedList::new(
                                "layout-animated-list",
                                self.layout_animated_list.clone(),
                            )
                            .child_keyed(
                                "design",
                                motion_item("Design", "Tokens and spacing", theme.tokens.primary),
                            )
                            .child_keyed(
                                "build",
                                motion_item("Build", "Component behavior", theme.tokens.success),
                            )
                            .child_keyed(
                                "review",
                                motion_item("Review", "Accessibility QA", theme.tokens.warning),
                            )
                            .child_keyed(
                                "document",
                                motion_item("Document", "Usage guidance", theme.tokens.accent),
                            )
                            .child_keyed(
                                "release",
                                motion_item("Release", "Publish package", theme.tokens.accent),
                            )
                            .gap(px(6.0)),
                        ),
                )
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .min_w(px(0.0))
                        .child(
                            row()
                                .items_center()
                                .justify_between()
                                .child(label_chip("LayoutTransition", &theme))
                                .child(
                                    Button::new("layout-transition-replay", "Replay")
                                        .variant(ButtonVariant::Ghost)
                                        .size(ButtonSize::Sm)
                                        .on_click(move |_, _, cx| {
                                            transition_view.update(cx, |this, cx| {
                                                this.layout_transition_version =
                                                    this.layout_transition_version.wrapping_add(1);
                                                cx.notify();
                                            });
                                        }),
                                ),
                        )
                        .child(
                            LayoutTransition::new("layout-transition-demo")
                                .version(self.layout_transition_version)
                                .animation(LayoutAnimation::FadeUp)
                                .stagger(std::time::Duration::from_millis(70))
                                .flex()
                                .flex_col()
                                .gap(px(6.0))
                                .child(motion_item(
                                    "Foundation",
                                    "Semantic structure",
                                    theme.tokens.primary,
                                ))
                                .child(motion_item(
                                    "Interaction",
                                    "Keyboard and pointer",
                                    theme.tokens.success,
                                ))
                                .child(motion_item(
                                    "Polish",
                                    "Motion and feedback",
                                    theme.tokens.warning,
                                )),
                        ),
                ),
        )
        .child(
            row()
                .items_stretch()
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .min_w(px(0.0))
                        .child(label_chip("SortableList", &theme))
                        .child(caption("Drag, or focus an item and press Alt + ↑ / ↓"))
                        .child(
                            kael_ui::components::sortable_list::SortableList::new(
                                self.layout_sortable.clone(),
                                move |item, index, dragging| {
                                    row()
                                        .items_center()
                                        .gap(px(10.0))
                                        .w_full()
                                        .p(px(10.0))
                                        .rounded(sortable_theme.tokens.radius_md)
                                        .border_1()
                                        .border_color(sortable_theme.tokens.border)
                                        .bg(sortable_theme.tokens.card)
                                        .opacity(if dragging { 0.55 } else { 1.0 })
                                        .child(Icon::new("grip-vertical").size(IconSize::Sm))
                                        .child(label(format!("{}. {}", index + 1, item)))
                                        .into_any_element()
                                },
                            )
                            .id("layout-sortable-list")
                            .gap(px(6.0)),
                        ),
                )
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .min_w(px(0.0))
                        .child(
                            row()
                                .items_center()
                                .justify_between()
                                .child(label_chip("InfiniteScroll", &theme))
                                .child(Badge::new(format!("{} rows", infinite_count))),
                        )
                        .child(
                            div()
                                .h(px(180.0))
                                .w_full()
                                .border_1()
                                .border_color(theme.tokens.border)
                                .rounded(theme.tokens.radius_lg)
                                .overflow_hidden()
                                .child(
                                    InfiniteScroll::new(self.layout_infinite.clone())
                                        .threshold(0.72)
                                        .on_load_more(move |_, _, cx| {
                                            infinite_view.update(cx, |this, cx| {
                                                this.layout_infinite_count =
                                                    (this.layout_infinite_count + 8).min(40);
                                                if this.layout_infinite_count >= 40 {
                                                    infinite_state.update(cx, |state, cx| {
                                                        state.set_end_reached();
                                                        cx.notify();
                                                    });
                                                } else {
                                                    infinite_state.update(cx, |state, cx| {
                                                        state.set_loaded();
                                                        cx.notify();
                                                    });
                                                }
                                                cx.notify();
                                            });
                                        })
                                        .children((0..infinite_count).map(|index| {
                                            row()
                                                .items_center()
                                                .justify_between()
                                                .px(px(12.0))
                                                .py(px(9.0))
                                                .border_b_1()
                                                .border_color(theme.tokens.border)
                                                .child(label(format!("Audit item {}", index + 1)))
                                                .child(caption(if index % 3 == 0 {
                                                    "Needs review"
                                                } else {
                                                    "Verified"
                                                }))
                                        })),
                                ),
                        ),
                ),
        )
        .child(
            row()
                .items_stretch()
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .min_w(px(0.0))
                        .child(
                            row()
                                .items_center()
                                .justify_between()
                                .child(label_chip("SharedElementTransition", &theme))
                                .child(caption(format!(
                                    "Progress {:.0}%",
                                    shared_progress * 100.0
                                ))),
                        )
                        .child(
                            div()
                                .relative()
                                .h(px(190.0))
                                .w_full()
                                .overflow_hidden()
                                .rounded(theme.tokens.radius_lg)
                                .border_1()
                                .border_color(theme.tokens.border)
                                .bg(theme.tokens.muted)
                                .child(
                                    div()
                                        .absolute()
                                        .left(px(14.0))
                                        .top(px(54.0))
                                        .size(px(10.0))
                                        .rounded_full()
                                        .bg(theme.tokens.muted_foreground.opacity(0.3)),
                                )
                                .child(
                                    div()
                                        .absolute()
                                        .right(px(14.0))
                                        .bottom(px(12.0))
                                        .size(px(10.0))
                                        .rounded_full()
                                        .bg(theme.tokens.muted_foreground.opacity(0.3)),
                                )
                                .child(
                                    SharedElementTransition::new(
                                        "layout-shared-element",
                                        self.layout_shared.clone(),
                                    )
                                    .content(
                                        div()
                                            .size_full()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .gap(px(6.0))
                                            .rounded(theme.tokens.radius_lg)
                                            .bg(theme.tokens.primary)
                                            .text_color(theme.tokens.primary_foreground)
                                            .child(
                                                Icon::new("sparkles")
                                                    .size(IconSize::Sm)
                                                    .color(theme.tokens.primary_foreground),
                                            )
                                            .child(
                                                label("Shared card".to_string())
                                                    .text_color(TextColor::Inherit),
                                            ),
                                    ),
                                )
                                .child(
                                    Button::new("layout-shared-move", "Move card")
                                        .variant(ButtonVariant::Secondary)
                                        .size(ButtonSize::Sm)
                                        .absolute()
                                        .left(px(150.0))
                                        .top(px(8.0))
                                        .on_click(move |_, _, cx| {
                                            shared_view.update(cx, |this, cx| {
                                                let target = if this.layout_shared_at_target {
                                                    Bounds::new(
                                                        point(px(14.0), px(54.0)),
                                                        size(px(124.0), px(74.0)),
                                                    )
                                                } else {
                                                    Bounds::new(
                                                        point(px(210.0), px(82.0)),
                                                        size(px(130.0), px(96.0)),
                                                    )
                                                };
                                                this.layout_shared_at_target =
                                                    !this.layout_shared_at_target;
                                                shared_state.update(cx, |state, cx| {
                                                    state.set_target_bounds(target);
                                                    state.transition_to(cx);
                                                });
                                                cx.notify();
                                            });
                                        }),
                                ),
                        ),
                )
                .child(
                    demo_surface(&theme)
                        .flex_1()
                        .min_w(px(0.0))
                        .child(
                            row()
                                .items_center()
                                .justify_between()
                                .child(label_chip("Ripple", &theme))
                                .child(
                                    Button::new("layout-ripple-replay", "Replay ripple")
                                        .variant(ButtonVariant::Ghost)
                                        .size(ButtonSize::Sm)
                                        .on_click(move |_, _, cx| {
                                            ripple_view.update(cx, |this, cx| {
                                                this.effects_ripple_version =
                                                    this.effects_ripple_version.wrapping_add(1);
                                                cx.notify();
                                            });
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .relative()
                                .h(px(190.0))
                                .w_full()
                                .overflow_hidden()
                                .rounded(theme.tokens.radius_lg)
                                .border_1()
                                .border_color(theme.tokens.border)
                                .bg(theme.tokens.accent)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    col()
                                        .items_center()
                                        .gap(px(4.0))
                                        .child(Icon::new("mouse-pointer-click"))
                                        .child(label("Contained feedback".to_string()))
                                        .child(caption("Replay to inspect the expansion")),
                                )
                                .child(
                                    Ripple::new(
                                        ("layout-ripple", ripple_version),
                                        point(px(200.0), px(95.0)),
                                        theme.tokens.primary,
                                    )
                                    .max_size(px(420.0)),
                                ),
                        ),
                ),
        )
        .child(
            demo_surface(&theme)
                .child(label_chip("Scrollable & Scrollbar", &theme))
                .child(caption(
                    "Two-axis overflow with persistent, bounded thumbs and a shared corner.",
                ))
                .child(
                    Scrollable::both(
                        col()
                            .w(px(980.0))
                            .h(px(360.0))
                            .p(px(14.0))
                            .gap(px(10.0))
                            .children((0..4).map(|row_index| {
                                row()
                                    .gap(px(10.0))
                                    .children((0..6).map(move |column_index| {
                                        let item = row_index * 6 + column_index + 1;
                                        col()
                                            .w(px(150.0))
                                            .h(px(72.0))
                                            .flex_shrink_0()
                                            .justify_between()
                                            .p(px(10.0))
                                            .rounded(theme.tokens.radius_md)
                                            .border_1()
                                            .border_color(theme.tokens.border)
                                            .bg(theme.tokens.card)
                                            .child(label(format!("Surface {item:02}")))
                                            .child(caption(if item % 3 == 0 {
                                                "Needs review"
                                            } else {
                                                "Verified"
                                            }))
                                    }))
                            })),
                    )
                    .id("layout-scrollable-both")
                    .scroll_size(size(px(980.0), px(360.0)))
                    .always_show_scrollbars()
                    .h(px(220.0))
                    .w_full()
                    .rounded(theme.tokens.radius_lg)
                    .border_1()
                    .border_color(theme.tokens.border),
                ),
        )
        .child(
            demo_surface(&theme)
                .child(label_chip("DrawerNavigation & SizeContext", &theme))
                .child(
                    row()
                        .items_center()
                        .justify_between()
                        .child(muted(
                            "A focus-managed navigation surface with explicit control density.",
                        ))
                        .child(
                            SizeContext::new(ControlSize::Sm).child(
                                Button::new("layout-drawer-open", "Open navigation drawer")
                                    .icon("panel-left")
                                    .size(ButtonSize::Sm)
                                    .on_click(move |_, _, cx| {
                                        drawer_state.update(cx, |state, cx| state.open(cx));
                                    }),
                            ),
                        ),
                ),
        );

        let active = self.category;
        let data_display_audit_section = std::env::var("ASTRYX_SHOWCASE_DATA_DISPLAY_SECTION").ok();
        let chart_audit_section = std::env::var("ASTRYX_SHOWCASE_CHART_SECTION").ok();
        let overlay_audit_section = std::env::var("ASTRYX_SHOWCASE_OVERLAY_SECTION").ok();
        let typography_audit_section = std::env::var("ASTRYX_SHOWCASE_TYPOGRAPHY_SECTION").ok();
        let media_audit_section = std::env::var("ASTRYX_SHOWCASE_MEDIA_SECTION").ok();
        let layout_audit_section = std::env::var("ASTRYX_SHOWCASE_LAYOUT_SECTION").ok();
        let layout_static_audit_section =
            std::env::var("ASTRYX_SHOWCASE_LAYOUT_STATIC_SECTION").ok();
        let feedback_audit_section = std::env::var("ASTRYX_SHOWCASE_FEEDBACK_SECTION").ok();
        let navigation_audit_section = std::env::var("ASTRYX_SHOWCASE_NAVIGATION_SECTION").ok();

        let sidebar = div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .w(px(216.0))
            .flex_shrink_0()
            .px(px(12.0))
            .py(px(16.0))
            .bg(theme.tokens.card)
            .border_r_1()
            .border_color(theme.tokens.border)
            .child(SideNavHeading::new("Components"))
            .children(ComponentCategory::ALL.into_iter().map(|cat| {
                let selected = cat == active;
                let view = view.clone();
                let nav_theme = theme.clone();
                kael::button(SharedString::from(format!("nav-{}", cat.id())))
                    .label(cat.label())
                    .on_click(move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.category = cat;
                            this.page_scroll.scroll_to_top_of_item(0);
                            cx.notify();
                        });
                    })
                    .render_with(move |state, _, _| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .w_full()
                            .h(px(36.0))
                            .px(px(10.0))
                            .rounded(nav_theme.tokens.radius_md)
                            .bg(if selected {
                                nav_theme.tokens.accent
                            } else {
                                transparent_black()
                            })
                            .when(state.focused, |this| {
                                this.shadow(smallvec::smallvec![astryx_focus_ring_outer(
                                    nav_theme.tokens.ring,
                                )])
                            })
                            .hover(|style| style.bg(nav_theme.tokens.accent))
                            .child(Icon::new(cat.icon()).size(IconSize::Sm).icon_color(
                                if selected {
                                    IconColor::Primary
                                } else {
                                    IconColor::Tertiary
                                },
                            ))
                            .child(
                                div()
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::Group)
                                            .states(AccessibilityState::HIDDEN),
                                    )
                                    .text_size(px(13.0))
                                    .font_weight(if selected {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::MEDIUM
                                    })
                                    .text_color(if selected {
                                        nav_theme.tokens.foreground
                                    } else {
                                        nav_theme.tokens.muted_foreground
                                    })
                                    .child(cat.label()),
                            )
                            .into_any_element()
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
                .child(inputs_tokenizer_typeahead)
                .child(inputs_structured)
                .child(inputs_editing),
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
                .when(
                    data_display_audit_section
                        .as_deref()
                        .is_none_or(|s| s == "overview"),
                    |this| this.child(badges).child(cards),
                )
                .when(
                    data_display_audit_section
                        .as_deref()
                        .is_none_or(|s| s == "tables"),
                    |this| this.child(data_table_section),
                )
                .when(
                    data_display_audit_section
                        .as_deref()
                        .is_none_or(|s| s == "rich-content"),
                    |this| this.child(rich_content_section),
                )
                .when(
                    data_display_audit_section
                        .as_deref()
                        .is_none_or(|s| s == "source"),
                    |this| this.child(source_renderer_section),
                )
                .when(
                    data_display_audit_section
                        .as_deref()
                        .is_none_or(|s| s == "grid"),
                    |this| this.child(data_grid_section),
                )
                .when(
                    data_display_audit_section
                        .as_deref()
                        .is_none_or(|s| s == "structured"),
                    |this| this.child(power_search_section),
                )
                .when(
                    data_display_audit_section
                        .as_deref()
                        .is_none_or(|s| s == "timeline"),
                    |this| this.child(timeline_sec),
                )
                .when(
                    data_display_audit_section
                        .as_deref()
                        .is_none_or(|s| s == "misc"),
                    |this| {
                        this.child(code_tags)
                            .child(dd_lists)
                            .child(dd_tree)
                            .child(dd_keys)
                            .child(dd_misc)
                    },
                ),
            ComponentCategory::Charts => col()
                .gap(px(20.0))
                .when(
                    chart_audit_section.as_deref().is_none_or(|s| s == "bar"),
                    |this| this.child(charts_bar),
                )
                .when(
                    chart_audit_section.as_deref().is_none_or(|s| s == "line"),
                    |this| this.child(charts_line),
                )
                .when(
                    chart_audit_section.as_deref().is_none_or(|s| s == "area"),
                    |this| this.child(charts_area),
                )
                .when(
                    chart_audit_section.as_deref().is_none_or(|s| s == "pie"),
                    |this| this.child(charts_pie_donut),
                )
                .when(
                    chart_audit_section.as_deref().is_none_or(|s| s == "gauge"),
                    |this| this.child(charts_gauge),
                )
                .when(
                    chart_audit_section
                        .as_deref()
                        .is_none_or(|s| s == "sparkline"),
                    |this| this.child(charts_sparkline),
                )
                .when(
                    chart_audit_section.as_deref().is_none_or(|s| s == "radar"),
                    |this| this.child(charts_radar),
                )
                .when(
                    chart_audit_section
                        .as_deref()
                        .is_none_or(|s| s == "heatmap"),
                    |this| this.child(charts_heatmap),
                )
                .when(
                    chart_audit_section
                        .as_deref()
                        .is_none_or(|s| s == "contributions"),
                    |this| this.child(charts_contributions),
                )
                .when(
                    chart_audit_section
                        .as_deref()
                        .is_none_or(|s| s == "treemap"),
                    |this| this.child(charts_treemap),
                ),
            ComponentCategory::Feedback => col()
                .gap(px(20.0))
                .when(
                    feedback_audit_section
                        .as_deref()
                        .is_none_or(|section| section == "overview" || section == "alerts"),
                    |this| this.child(feedback),
                )
                .when(
                    feedback_audit_section
                        .as_deref()
                        .is_none_or(|section| section == "overview" || section == "banners"),
                    |this| this.child(extras),
                )
                .when(
                    feedback_audit_section
                        .as_deref()
                        .is_none_or(|section| section == "overview" || section == "misc"),
                    |this| this.child(misc),
                )
                .when(
                    feedback_audit_section
                        .as_deref()
                        .is_none_or(|section| section == "overview" || section == "empty"),
                    |this| this.child(empty_disclosure),
                )
                .when(
                    feedback_audit_section
                        .as_deref()
                        .is_none_or(|section| section == "progress"),
                    |this| this.child(fb_circular).child(fb_animated_progress),
                )
                .when(
                    feedback_audit_section
                        .as_deref()
                        .is_none_or(|section| section == "numbers"),
                    |this| this.child(fb_numbers),
                )
                .when(
                    feedback_audit_section
                        .as_deref()
                        .is_none_or(|section| section == "indicators"),
                    |this| this.child(fb_indicators),
                )
                .when(
                    feedback_audit_section
                        .as_deref()
                        .is_none_or(|section| section == "loading"),
                    |this| this.child(fb_loading),
                ),
            ComponentCategory::Navigation => col()
                .gap(px(20.0))
                .when(
                    navigation_audit_section
                        .as_deref()
                        .is_none_or(|section| section == "tabs"),
                    |this| this.child(nav_sec),
                )
                .when(
                    navigation_audit_section
                        .as_deref()
                        .is_none_or(|section| section == "disclosure"),
                    |this| this.child(nav_disclosure),
                )
                .when(
                    navigation_audit_section
                        .as_deref()
                        .is_none_or(|section| section == "foundations"),
                    |this| this.child(nav_foundations),
                )
                .when(
                    navigation_audit_section
                        .as_deref()
                        .is_none_or(|section| section == "menus"),
                    |this| this.child(nav_menus),
                )
                .when(
                    navigation_audit_section
                        .as_deref()
                        .is_none_or(|section| section == "toolbar"),
                    |this| this.child(nav_toolbar),
                )
                .when(
                    navigation_audit_section
                        .as_deref()
                        .is_none_or(|section| section == "chrome"),
                    |this| this.child(nav_chrome),
                )
                .when(
                    navigation_audit_section
                        .as_deref()
                        .is_none_or(|section| section == "mobile"),
                    |this| this.child(nav_mobile),
                ),
            ComponentCategory::Overlays => col()
                .gap(px(20.0))
                .when(
                    overlay_audit_section
                        .as_deref()
                        .is_none_or(|section| section == "preview"),
                    |this| this.child(overlays),
                )
                .when(
                    overlay_audit_section
                        .as_deref()
                        .is_none_or(|section| section == "triggers"),
                    |this| this.child(modal_triggers),
                )
                .when(
                    overlay_audit_section
                        .as_deref()
                        .is_none_or(|section| section == "alert"),
                    |this| this.child(overlays_alert),
                )
                .when(
                    overlay_audit_section
                        .as_deref()
                        .is_none_or(|section| section == "context-menu"),
                    |this| this.child(overlays_context_menu),
                )
                .when(
                    overlay_audit_section
                        .as_deref()
                        .is_none_or(|section| section == "tooltip"),
                    |this| this.child(overlays_tooltip),
                ),
            ComponentCategory::Typography => col()
                .gap(px(20.0))
                .when(
                    typography_audit_section
                        .as_deref()
                        .is_none_or(|section| matches!(section, "base" | "static")),
                    |this| this.child(typography),
                )
                .when(
                    typography_audit_section
                        .as_deref()
                        .is_none_or(|section| matches!(section, "headings" | "static")),
                    |this| this.child(typography_heading),
                )
                .when(
                    typography_audit_section
                        .as_deref()
                        .is_none_or(|section| matches!(section, "quote" | "static")),
                    |this| this.child(typography_quote_gradient),
                )
                .when(
                    typography_audit_section
                        .as_deref()
                        .is_none_or(|section| matches!(section, "links" | "static")),
                    |this| this.child(typography_links),
                )
                .when(
                    typography_audit_section
                        .as_deref()
                        .is_none_or(|section| matches!(section, "code" | "static")),
                    |this| this.child(typography_code),
                )
                .when(
                    typography_audit_section
                        .as_deref()
                        .is_none_or(|section| matches!(section, "kbd" | "static")),
                    |this| this.child(typography_kbd),
                )
                .when(
                    typography_audit_section
                        .as_deref()
                        .is_none_or(|section| section == "motion"),
                    |this| this.child(typography_motion),
                ),
            ComponentCategory::Media => col()
                .gap(px(20.0))
                .when(
                    media_audit_section
                        .as_deref()
                        .is_none_or(|section| matches!(section, "icons" | "static")),
                    |this| this.child(media_icons),
                )
                .when(
                    media_audit_section
                        .as_deref()
                        .is_none_or(|section| matches!(section, "avatars" | "static")),
                    |this| this.child(media_avatars),
                )
                .when(
                    media_audit_section
                        .as_deref()
                        .is_none_or(|section| matches!(section, "thumbnails" | "static")),
                    |this| this.child(media_thumbnails),
                )
                .when(
                    media_audit_section
                        .as_deref()
                        .is_none_or(|section| matches!(section, "waveform" | "static")),
                    |this| this.child(media_waveform),
                )
                .when(
                    media_audit_section
                        .as_deref()
                        .is_none_or(|section| matches!(section, "players" | "static")),
                    |this| this.child(media_players),
                )
                .when(
                    media_audit_section
                        .as_deref()
                        .is_none_or(|section| matches!(section, "viewer" | "static")),
                    |this| this.child(media_viewer),
                )
                .when(
                    media_audit_section
                        .as_deref()
                        .is_none_or(|section| matches!(section, "surfaces" | "static")),
                    |this| this.child(media_surfaces),
                )
                .when(
                    media_audit_section
                        .as_deref()
                        .is_none_or(|section| matches!(section, "layout" | "static")),
                    |this| this.child(media_layout),
                )
                .when(
                    media_audit_section
                        .as_deref()
                        .is_none_or(|section| matches!(section, "gradient" | "static")),
                    |this| this.child(media_gradient_text),
                )
                .when(
                    media_audit_section
                        .as_deref()
                        .is_none_or(|section| section == "effects"),
                    |this| this.child(media_effects),
                )
                .when(
                    media_audit_section
                        .as_deref()
                        .is_none_or(|section| section == "interactive"),
                    |this| this.child(media_interactive_effects),
                ),
            ComponentCategory::Layout => col()
                .gap(px(20.0))
                .when(
                    layout_audit_section
                        .as_deref()
                        .is_none_or(|section| section == "static"),
                    |this| {
                        this.when(
                            layout_static_audit_section
                                .as_deref()
                                .is_none_or(|section| section == "foundations"),
                            |this| {
                                this.child(layout_primitives_section)
                                    .child(layout_stack_section)
                            },
                        )
                        .when(
                            layout_static_audit_section
                                .as_deref()
                                .is_none_or(|section| section == "grid"),
                            |this| this.child(layout_grid_section),
                        )
                        .when(
                            layout_static_audit_section
                                .as_deref()
                                .is_none_or(|section| section == "separator"),
                            |this| this.child(layout_separator_section),
                        )
                        .when(
                            layout_static_audit_section
                                .as_deref()
                                .is_none_or(|section| section == "collapsible"),
                            |this| this.child(layout_collapsible_section),
                        )
                        .when(
                            layout_static_audit_section
                                .as_deref()
                                .is_none_or(|section| section == "panes"),
                            |this| this.child(layout_panes_section),
                        )
                    },
                )
                .when(
                    layout_audit_section
                        .as_deref()
                        .is_none_or(|section| section == "motion"),
                    |this| this.child(layout_motion_section),
                ),
        };

        let page = col()
            .w_full()
            .max_w(px(1120.0))
            .mx_auto()
            .gap(px(20.0))
            .p(px(24.0))
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
                    .bg(theme.tokens.background)
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
                    .child(
                        scrollable_vertical(page)
                            .with_scroll_handle(self.page_scroll.clone())
                            .size_full(),
                    ),
            );

        let bottom_sheet = self.show_bottom_sheet.then(|| {
            let close_view = view.clone();
            let action_view = view.clone();
            BottomSheet::new()
                .id("astryx-bottom-sheet")
                .size(BottomSheetSize::Md)
                .title("Review component changes")
                .description("A mobile-friendly secondary task that keeps the workspace nearby.")
                .content(
                    col()
                        .gap(px(16.0))
                        .p(px(24.0))
                        .child(
                            Alert::info()
                                .title("Accessibility checks")
                                .description("Keyboard and screen-reader review is ready."),
                        )
                        .child(
                            MetadataList::new()
                                .columns(MetadataListColumns::Count(2))
                                .item(MetadataListItem::new("Focus order", "Verified"))
                                .item(MetadataListItem::new("Escape dismissal", "Enabled"))
                                .item(MetadataListItem::new("Backdrop dismissal", "Enabled"))
                                .item(MetadataListItem::new("Scroll region", "Independent")),
                        ),
                )
                .actions(
                    Button::new("bottom-sheet-save", "Save review")
                        .size(ButtonSize::Sm)
                        .on_click(move |_, window, cx| {
                            action_view.update(cx, |this, cx| {
                                this.toast_n += 1;
                                let id = this.toast_n;
                                this.toasts.update(cx, |manager, cx| {
                                    manager.add_toast(
                                        ToastItem::new(id, "Review saved")
                                            .variant(ToastVariant::Success),
                                        window,
                                        cx,
                                    );
                                });
                            });
                        }),
                )
                .on_close(move |_, cx| {
                    close_view.update(cx, |this, cx| {
                        this.show_bottom_sheet = false;
                        cx.notify();
                    });
                })
        });

        let popover_menu = self.show_popover_menu.then(|| {
            let close_view = view.clone();
            let rename_view = view.clone();
            let duplicate_view = view.clone();
            PopoverMenu::new(
                self.popover_menu_position,
                vec![
                    PopoverMenuItem::new("rename", "Rename component")
                        .icon("pencil")
                        .shortcut("⌘R")
                        .on_click(move |window, cx| {
                            rename_view.update(cx, |this, cx| {
                                this.toast_n += 1;
                                let id = this.toast_n;
                                this.toasts.update(cx, |manager, cx| {
                                    manager.add_toast(
                                        ToastItem::new(id, "Rename selected"),
                                        window,
                                        cx,
                                    );
                                });
                            });
                        }),
                    PopoverMenuItem::new("duplicate", "Duplicate")
                        .icon("copy")
                        .description("Create a copy beside the current component")
                        .shortcut("⌘D")
                        .on_click(move |window, cx| {
                            duplicate_view.update(cx, |this, cx| {
                                this.toast_n += 1;
                                let id = this.toast_n;
                                this.toasts.update(cx, |manager, cx| {
                                    manager.add_toast(
                                        ToastItem::new(id, "Component duplicated")
                                            .variant(ToastVariant::Success),
                                        window,
                                        cx,
                                    );
                                });
                            });
                        }),
                    PopoverMenuItem::new("archive", "Archive")
                        .icon("archive")
                        .description("Unavailable while the component is published")
                        .disabled(true),
                ],
            )
            .id("astryx-popover-menu")
            .on_close(move |_, cx| {
                close_view.update(cx, |this, cx| {
                    this.show_popover_menu = false;
                    cx.notify();
                });
            })
        });

        let drawer_close_state = self.layout_drawer.clone();
        let layout_drawer =
            DrawerNavigation::new("astryx-layout-drawer", self.layout_drawer.clone())
                .width(px(320.0))
                .child(
                    col()
                        .size_full()
                        .child(
                            row()
                                .items_center()
                                .justify_between()
                                .px(px(18.0))
                                .py(px(16.0))
                                .border_b_1()
                                .border_color(theme.tokens.border)
                                .child(
                                    col()
                                        .gap(px(2.0))
                                        .child(h4("Workspace".to_string()))
                                        .child(caption("Navigation drawer")),
                                )
                                .child(
                                    IconButton::new("x")
                                        .label("Close navigation drawer")
                                        .on_click(move |_, _, cx| {
                                            drawer_close_state
                                                .update(cx, |state, cx| state.close(cx));
                                        }),
                                ),
                        )
                        .child(
                            col()
                                .gap(px(4.0))
                                .p(px(12.0))
                                .child(
                                    NavItem::new("Overview")
                                        .icon("layout-dashboard")
                                        .selected(true),
                                )
                                .child(NavItem::new("Components").icon("blocks"))
                                .child(NavItem::new("Accessibility").icon("accessibility"))
                                .child(NavItem::new("Release checks").icon("check-circle")),
                        )
                        .child(
                            div()
                                .mt_auto()
                                .p(px(16.0))
                                .border_t_1()
                                .border_color(theme.tokens.border)
                                .child(
                                    Alert::info()
                                        .title("Focus is contained")
                                        .description("Press Escape to return to the layout page."),
                                ),
                        ),
                );

        div()
            .relative()
            .size_full()
            .on_action(cx.listener(|this, _: &ShowNavigation, _, cx| {
                this.category = ComponentCategory::Navigation;
                this.page_scroll.scroll_to_top_of_item(0);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ShowOverlays, _, cx| {
                this.category = ComponentCategory::Overlays;
                this.page_scroll.scroll_to_top_of_item(0);
                cx.notify();
            }))
            .child(main)
            .when(self.show_dialog, |this| this.child(self.dialog.clone()))
            .when(self.show_sheet, |this| this.child(self.sheet.clone()))
            .when(self.show_alert_dialog, |this| {
                this.child(self.overlays_alert_dialog.clone())
            })
            .when(self.show_command_palette, |this| {
                this.child(self.overlays_command_palette.clone())
            })
            .when(self.show_media_viewer, |this| {
                this.child(self.media_viewer.clone())
            })
            .when_some(bottom_sheet, |this, sheet| this.child(sheet))
            .when_some(popover_menu, |this, menu| this.child(menu))
            .child(layout_drawer)
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

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let app_id = std::env::var("ASTRYX_SHOWCASE_APP_ID")
        .unwrap_or_else(|_| "dev.kael.astryx-showcase".to_string());
    Application::try_new()?
        .with_assets(Assets {
            base: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        })
        .run(move |cx| {
            kael_ui::init(cx);
            kael_ui::set_icon_base_path("assets/icons");
            install_theme(cx, Theme::astryx_neutral());
            cx.on_action(|_: &QuitShowcase, cx| cx.quit());
            cx.bind_keys([
                KeyBinding::new("cmd-1", ShowNavigation, None),
                KeyBinding::new("cmd-2", ShowOverlays, None),
                KeyBinding::new("cmd-q", QuitShowcase, None),
            ]);
            cx.set_menus(
                StandardMacMenuBar::new("Astryx Showcase")
                    .file_menu(file_menu().action("Quit Astryx Showcase", QuitShowcase))
                    .view_menu(
                        view_menu()
                            .action("Navigation Components", ShowNavigation)
                            .action("Overlay Components", ShowOverlays),
                    )
                    .build(),
            );

            let bounds = Bounds::centered(None, size(px(1200.0), px(860.0)), cx);
            if let Err(error) = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(900.0), px(640.0))),
                    app_id: Some(app_id),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Astryx · Kael UI".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |_, cx| cx.new(AstryxShowcase::new),
            ) {
                eprintln!("failed to open the Astryx showcase window: {error}");
                cx.quit();
            }
        });
    Ok(())
}

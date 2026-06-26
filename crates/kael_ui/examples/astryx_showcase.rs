use kael::{prelude::FluentBuilder as _, *};
use kael_ui::astryx::ControlSize;
use kael_ui::components::alert::Alert;
use kael_ui::components::button_group::{ButtonGroup, ButtonGroupItem};
use kael_ui::components::code_block::CodeBlock;
use kael_ui::components::color_picker::{ColorPicker, ColorPickerState};
use kael_ui::components::date_picker::{DatePicker, DatePickerState};
use kael_ui::components::file_upload::{FileUpload, FileUploadState};
use kael_ui::components::number_input::{NumberInput, NumberInputState};
use kael_ui::components::otp_input::{OTPInput, OTPState};
use kael_ui::components::pagination::Pagination;
use kael_ui::components::rating::{Rating, RatingState};
use kael_ui::components::scrollable::scrollable_vertical;
use kael_ui::components::segmented_nav::{SegmentedNav, SegmentedNavState};
use kael_ui::components::select::{Select, SelectOption};
use kael_ui::components::slider::{Slider, SliderState};
use kael_ui::components::stepper::{StepItem, Stepper, StepperState};
use kael_ui::components::tag_input::{TagInput, TagInputState};
use kael_ui::components::text::{body, caption, code, h1, h2, h3, h4, h5, h6, label, muted};
use kael_ui::components::time_picker::{TimePicker, TimePickerState};
use kael_ui::components::toggle_group::{ToggleGroup, ToggleGroupItem, ToggleGroupVariant};
use kael_ui::components::tooltip::tooltip;
use kael_ui::display::accordion::Accordion;
use kael_ui::display::table::{Table, TableColumn, TableRow};
use kael_ui::navigation::tabs::{TabItem, TabVariant, Tabs};
use kael_ui::overlays::hover_card::HoverCard;
use kael_ui::overlays::popover::{Popover, PopoverContent};
use kael_ui::prelude::{
    Avatar, AvatarGroup, AvatarItem, AvatarSize, Badge, BadgeVariant, Banner, Button, ButtonSize,
    ButtonVariant, Card, Checkbox, Collapsible, EmptyState, Hue, IconButton, ProgressBar, Radio,
    RadioGroup, SelectableCard, Separator, Skeleton, SkeletonVariant, Spinner, SpinnerSize,
    SpinnerVariant, StatusDot, StatusTone, TextField, TextFieldSize, Textarea, Timeline,
    TimelineItem, Toggle, KBD,
};
use kael_ui::theme::{install_theme, use_theme, Theme, ThemeTokens, ThemeVariant};

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
}

impl AstryxShowcase {
    fn new(cx: &mut Context<Self>) -> Self {
        let slider = cx.new(|cx| {
            let mut s = SliderState::new(cx);
            s.set_value(60.0, cx);
            s
        });
        let select = cx.new(|cx| {
            Select::new(cx)
                .placeholder("Select a country")
                .options(vec![
                    SelectOption::new("us".to_string(), "United States"),
                    SelectOption::new("gh".to_string(), "Ghana"),
                    SelectOption::new("jp".to_string(), "Japan"),
                    SelectOption::new("se".to_string(), "Sweden"),
                ])
        });
        let stepper = cx.new(|cx| {
            StepperState::new(cx).with_steps(vec![
                StepItem::new("Account"),
                StepItem::new("Profile"),
                StepItem::new("Confirm"),
            ])
        });
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

        let inputs = section("Text inputs", "Sizes, focus rings and validation", &theme).child(
            row()
                .items_end()
                .child(
                    col().child(label_chip("Small", &theme)).child(
                        div().w(px(200.0)).child(
                            TextField::new(cx)
                                .size(TextFieldSize::Sm)
                                .placeholder("Search…"),
                        ),
                    ),
                )
                .child(
                    col().child(label_chip("Medium", &theme)).child(
                        div()
                            .w(px(220.0))
                            .child(TextField::new(cx).placeholder("you@example.com")),
                    ),
                )
                .child(
                    col().child(label_chip("Invalid", &theme)).child(
                        div().w(px(220.0)).child(
                            TextField::new(cx)
                                .invalid(true)
                                .placeholder("Required field"),
                        ),
                    ),
                )
                .child(
                    col().child(label_chip("Disabled", &theme)).child(
                        div()
                            .w(px(200.0))
                            .child(TextField::new(cx).disabled(true).placeholder("Disabled")),
                    ),
                ),
        );

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
        .child(Banner::success("All systems operational."))
        .child(
            row()
                .gap(px(24.0))
                .child(StatusDot::success().label("Online").pulse(true))
                .child(StatusDot::new(StatusTone::Warning).label("Degraded"))
                .child(StatusDot::error().label("Offline"))
                .child(StatusDot::new(StatusTone::Hue(Hue::Purple)).label("Beta")),
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
                .child(div().w(px(220.0)).child(ProgressBar::new(0.62)))
                .child(Spinner::new().size(SpinnerSize::Md))
                .child(
                    Spinner::new()
                        .size(SpinnerSize::Md)
                        .variant(SpinnerVariant::Primary),
                )
                .child(
                    row()
                        .gap(px(8.0))
                        .child(Avatar::new().name("Augustus Otu").size(AvatarSize::Md))
                        .child(Avatar::new().name("Kael UI").size(AvatarSize::Md))
                        .child(Avatar::new().name("Astryx").size(AvatarSize::Md)),
                )
                .child(row().gap(px(6.0)).child(KBD::new("⌘")).child(KBD::new("K"))),
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
                    .child(code("let astryx = Theme::astryx();".to_string())),
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
                .child(tooltip(
                    Button::new("tt-btn", "Hover for tooltip").variant(ButtonVariant::Outline),
                    "Astryx-styled tooltip",
                )),
        )
        .child(
            col()
                .gap(px(10.0))
                .child(
                    Skeleton::new()
                        .variant(SkeletonVariant::Text)
                        .w(px(260.0))
                        .h(px(12.0)),
                )
                .child(
                    Skeleton::new()
                        .variant(SkeletonVariant::Text)
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
                .item("grid", "Grid")
                .item("list", "List")
                .item("table", "Table"),
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
        .child(div().w(px(280.0)).child(Slider::new(self.slider.clone())));

        let more_inputs = section("More inputs", "Textarea and icon buttons", &theme)
            .child(
                div().w(px(300.0)).child(
                    Textarea::new("ta-demo")
                        .placeholder("Write a description…")
                        .rows(3),
                ),
            )
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
                    .tabs(vec![
                        TabItem::new("home", "Home"),
                        TabItem::new("projects", "Projects"),
                        TabItem::new("settings", "Settings"),
                    ])
                    .selected_index(0),
            )
            .child(
                Tabs::new()
                    .variant(TabVariant::Pills)
                    .tabs(vec![
                        TabItem::new("day", "Day"),
                        TabItem::new("week", "Week"),
                        TabItem::new("month", "Month"),
                    ])
                    .selected_index(1),
            );

        let data_table = section("Data table", "Headers, rows and dividers", &theme).child(
            Table::new()
                .columns(vec![
                    TableColumn::new("Name").width(px(160.0)),
                    TableColumn::new("Role").width(px(110.0)),
                    TableColumn::new("Status").width(px(110.0)),
                ])
                .rows(vec![
                    TableRow::new(vec!["Augustus Otu".into(), "Owner".into(), "Active".into()]),
                    TableRow::new(vec!["Kael UI".into(), "Editor".into(), "Active".into()]),
                    TableRow::new(vec!["Astryx".into(), "Viewer".into(), "Invited".into()]),
                ]),
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
            div()
                .w(px(160.0))
                .child(NumberInput::new(self.number.clone())),
        );

        let rating_stepper = section("Rating & stepper", "Feedback and multi-step flows", &theme)
            .child(Rating::new(self.rating.clone()))
            .child(Stepper::new(self.stepper.clone()));

        let otp_date = section("OTP & date picker", "Specialized inputs", &theme)
            .child(OTPInput::new(&self.otp))
            .child(div().w(px(220.0)).child(DatePicker::new(self.date.clone())));

        let pickers = section("Pickers", "Time, color and file upload", &theme)
            .child(
                div()
                    .w(px(200.0))
                    .child(TimePicker::new(self.time_state.clone())),
            )
            .child(ColorPicker::new("cp-demo", self.color_state.clone()))
            .child(
                div()
                    .w(px(340.0))
                    .child(FileUpload::new("fu-demo", self.file_state.clone())),
            );

        let overlays = section("Overlays", "Hover card and popover", &theme).child(
            row()
                .child(
                    HoverCard::new()
                        .trigger(Button::new("hc", "Hover card").variant(ButtonVariant::Outline))
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
                            Button::new("pop-t", "Open popover").variant(ButtonVariant::Outline),
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
        );

        let code_tags = section("Code, tags & toggle group", "Tokens and snippets", &theme)
            .child(
                CodeBlock::new("let astryx = Theme::astryx_neutral();\ninstall_theme(cx, astryx);")
                    .language("rust"),
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

        let col_a = div()
            .flex()
            .flex_col()
            .flex_1()
            .gap(px(20.0))
            .child(buttons)
            .child(badges)
            .child(inputs)
            .child(more_inputs)
            .child(dropdowns)
            .child(otp_date)
            .child(pickers)
            .child(selection)
            .child(controls)
            .child(nav_sec)
            .child(feedback)
            .child(nav_disclosure);

        let col_b = div()
            .flex()
            .flex_col()
            .flex_1()
            .gap(px(20.0))
            .child(typography)
            .child(extras)
            .child(misc)
            .child(details)
            .child(rating_stepper)
            .child(code_tags)
            .child(overlays)
            .child(data_table)
            .child(timeline_sec)
            .child(empty_disclosure)
            .child(cards);

        let content = div()
            .flex()
            .flex_col()
            .gap(px(20.0))
            .p(px(24.0))
            .pb(px(64.0))
            .child(header)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(20.0))
                    .items_start()
                    .child(col_a)
                    .child(col_b),
            );

        div()
            .size_full()
            .bg(theme.tokens.background)
            .text_color(theme.tokens.foreground)
            .font_family(theme.tokens.font_family.clone())
            .child(scrollable_vertical(content).size_full())
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

use kael::{prelude::FluentBuilder as _, *};
use kael_ui::components::alert::Alert;
use kael_ui::components::scrollable::scrollable_vertical;
use kael_ui::components::text::body;
use kael_ui::components::text::{caption, h1, h3, muted};
use kael_ui::prelude::{
    Avatar, AvatarSize, Badge, BadgeVariant, Button, ButtonSize, ButtonVariant, Card, Checkbox,
    Hue, ProgressBar, Radio, RadioGroup, Spinner, SpinnerSize, SpinnerVariant, TextField,
    TextFieldSize, Toggle, KBD,
};
use kael_ui::theme::{install_theme, use_theme, Theme, ThemeTokens, ThemeVariant};

struct AstryxShowcase {
    terms: bool,
    notifications: bool,
    marketing: bool,
    plan: usize,
}

impl AstryxShowcase {
    fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            terms: true,
            notifications: true,
            marketing: false,
            plan: 1,
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
                        "Astryx",
                        variant == ThemeVariant::Astryx,
                        ThemeTokens::astryx(),
                        &theme,
                        |cx| install_theme(cx, Theme::astryx()),
                    ))
                    .child(theme_pill(
                        "Astryx Dark",
                        variant == ThemeVariant::AstryxDark,
                        ThemeTokens::astryx_dark(),
                        &theme,
                        |cx| install_theme(cx, Theme::astryx_dark()),
                    ))
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

        let content = div()
            .flex()
            .flex_col()
            .gap(px(20.0))
            .p(px(24.0))
            .pb(px(64.0))
            .child(header)
            .child(buttons)
            .child(badges)
            .child(inputs)
            .child(selection)
            .child(feedback)
            .child(misc)
            .child(cards);

        div()
            .size_full()
            .bg(theme.tokens.background)
            .text_color(theme.tokens.foreground)
            .font_family(theme.tokens.font_family.clone())
            .child(scrollable_vertical(content).size_full())
    }
}

fn main() {
    Application::new().run(move |cx| {
        let bounds = Bounds::centered(None, size(px(1040.0), px(860.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Astryx · Kael UI".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| {
                install_theme(cx, Theme::astryx());
                cx.new(AstryxShowcase::new)
            },
        )
        .unwrap();
    });
}

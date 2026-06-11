//! Analytics dashboard template: sidebar navigation, stat cards, charts, and a
//! live data table — everything an Electron dashboard does, fully native.

use kael_ui::components::icon_source::IconSource;
use kael_ui::components::input::{Input, InputSize, InputState};
use kael_ui::components::input_state::InputEvent;
use kael_ui::components::scrollable::scrollable_vertical;
use kael_ui::navigation::sidebar::{Sidebar, SidebarItem, SidebarVariant};
use kael_ui::prelude::*;
use std::path::PathBuf;

struct Assets {
    base: PathBuf,
}

impl AssetSource for Assets {
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

#[derive(Clone)]
struct Order {
    id: u32,
    customer: String,
    product: String,
    amount: String,
    status: String,
}

fn sample_orders() -> Vec<Order> {
    [
        (
            1042,
            "Ada Lovelace",
            "Pro Plan (Annual)",
            "$1,188.00",
            "Paid",
        ),
        (1041, "Grace Hopper", "Team Plan", "$490.00", "Paid"),
        (1040, "Alan Turing", "Pro Plan", "$99.00", "Pending"),
        (1039, "Katherine Johnson", "Enterprise", "$4,800.00", "Paid"),
        (1038, "Linus Torvalds", "Team Plan", "$490.00", "Refunded"),
        (
            1037,
            "Margaret Hamilton",
            "Pro Plan (Annual)",
            "$1,188.00",
            "Paid",
        ),
        (1036, "Dennis Ritchie", "Starter", "$29.00", "Paid"),
        (1035, "Barbara Liskov", "Enterprise", "$4,800.00", "Pending"),
    ]
    .into_iter()
    .map(|(id, customer, product, amount, status)| Order {
        id,
        customer: customer.into(),
        product: product.into(),
        amount: amount.into(),
        status: status.into(),
    })
    .collect()
}

fn main() {
    Application::new()
        .with_assets(Assets {
            base: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/kael_ui"),
        })
        .run(|cx| {
            kael_ui::init(cx);
            kael_ui::set_icon_base_path("assets/icons");
            install_theme(cx, Theme::dark());

            cx.open_window(
                WindowOptions {
                    titlebar: Some(TitlebarOptions {
                        title: Some("Acme Analytics".into()),
                        ..Default::default()
                    }),
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1380.0), px(900.0)),
                    })),
                    ..Default::default()
                },
                |_, cx| cx.new(DashboardApp::new),
            )
            .unwrap();
            cx.activate(true);
        });
}

struct DashboardApp {
    search: Entity<InputState>,
    orders: Entity<DataTable<Order>>,
    section: String,
}

impl DashboardApp {
    fn new(cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(cx).placeholder("Search orders, customers…"));
        let orders = cx.new(|cx| {
            DataTable::new(sample_orders(), Self::order_columns(), cx).sticky_header(true)
        });

        cx.subscribe(&search, |this: &mut Self, search, event, cx| {
            if !matches!(event, InputEvent::Change) {
                return;
            }
            let query = search.read(cx).content().to_lowercase();
            let filtered: Vec<Order> = sample_orders()
                .into_iter()
                .filter(|order| {
                    query.is_empty()
                        || order.customer.to_lowercase().contains(&query)
                        || order.product.to_lowercase().contains(&query)
                        || format!("#{}", order.id).contains(&query)
                })
                .collect();
            this.orders
                .update(cx, |table, cx| table.set_data(filtered, cx));
        })
        .detach();

        Self {
            search,
            orders,
            section: "dashboard".into(),
        }
    }

    fn order_columns() -> Vec<ColumnDef<Order>> {
        vec![
            ColumnDef::new("id", "Order", |o: &Order| format!("#{}", o.id).into()).width(px(90.0)),
            ColumnDef::new("customer", "Customer", |o: &Order| {
                o.customer.clone().into()
            })
            .width(px(220.0)),
            ColumnDef::new("product", "Product", |o: &Order| o.product.clone().into())
                .width(px(220.0)),
            ColumnDef::new("amount", "Amount", |o: &Order| o.amount.clone().into())
                .width(px(130.0)),
            ColumnDef::new("status", "Status", |o: &Order| o.status.clone().into())
                .width(px(120.0)),
        ]
    }

    fn stat_card(
        &self,
        title: &'static str,
        value: &'static str,
        delta: &'static str,
        positive: bool,
        data: Vec<f64>,
        tokens: &ThemeTokens,
    ) -> impl IntoElement {
        let trend_color = if positive {
            hsla(152.0 / 360.0, 0.69, 0.45, 1.0)
        } else {
            tokens.destructive
        };
        Card::new()
            .content(
                VStack::new()
                    .gap(px(10.0))
                    .child(
                        HStack::new()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .text_color(tokens.muted_foreground)
                                    .child(title),
                            )
                            .child(Badge::new(delta).variant(if positive {
                                BadgeVariant::Secondary
                            } else {
                                BadgeVariant::Destructive
                            })),
                    )
                    .child(
                        div()
                            .text_size(px(28.0))
                            .font_weight(FontWeight::BOLD)
                            .child(value),
                    )
                    .child(
                        Sparkline::area(data)
                            .width(px(240.0))
                            .height(px(36.0))
                            .line_color(trend_color),
                    ),
            )
            .flex_1()
    }

    fn revenue_chart(&self, tokens: &ThemeTokens) -> impl IntoElement {
        let revenue = vec![
            (0.0, 42.0, "Jan"),
            (1.0, 49.0, "Feb"),
            (2.0, 47.0, "Mar"),
            (3.0, 58.0, "Apr"),
            (4.0, 64.0, "May"),
            (5.0, 61.0, "Jun"),
            (6.0, 72.0, "Jul"),
            (7.0, 78.0, "Aug"),
            (8.0, 84.0, "Sep"),
            (9.0, 91.0, "Oct"),
            (10.0, 97.0, "Nov"),
            (11.0, 108.0, "Dec"),
        ];
        Card::new()
            .header(
                HStack::new()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_size(px(15.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Revenue (last 12 months)"),
                    )
                    .child(Badge::new("+18.2% YoY").variant(BadgeVariant::Outline)),
            )
            .content(
                div().h(px(260.0)).w_full().child(
                    LineChart::single(
                        LineChartSeries::new(
                            "Revenue ($k)",
                            revenue
                                .iter()
                                .map(|(x, y, label)| LineChartPoint::new(*x, *y).label(*label))
                                .collect(),
                        )
                        .color(tokens.primary)
                        .fill_area(true),
                    )
                    .smooth(true)
                    .show_grid(true)
                    .x_labels(revenue.iter().map(|(_, _, l)| *l).collect::<Vec<_>>()),
                ),
            )
            .flex_1()
    }

    fn regions_chart(&self) -> impl IntoElement {
        Card::new()
            .header(
                div()
                    .text_size(px(15.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Sales by region"),
            )
            .content(
                div().h(px(260.0)).w_full().child(
                    BarChart::new(vec![
                        BarChartData::new("NA", 84.0).color(rgb(0x60a5fa).into()),
                        BarChartData::new("EU", 67.0).color(rgb(0x818cf8).into()),
                        BarChartData::new("APAC", 52.0).color(rgb(0x34d399).into()),
                        BarChartData::new("LATAM", 31.0).color(rgb(0xfbbf24).into()),
                        BarChartData::new("MEA", 18.0).color(rgb(0xf87171).into()),
                    ])
                    .show_values(true)
                    .chart_height(px(220.0)),
                ),
            )
            .w(px(420.0))
    }
}

impl Render for DashboardApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = use_theme();
        let tokens = theme.tokens.clone();

        let sidebar = Sidebar::new(cx)
            .items(vec![
                SidebarItem::new("dashboard".to_string(), "Dashboard")
                    .with_icon(IconSource::Named("layout-dashboard".to_string())),
                SidebarItem::new("analytics".to_string(), "Analytics")
                    .with_icon(IconSource::Named("chart-line".to_string())),
                SidebarItem::new("customers".to_string(), "Customers")
                    .with_icon(IconSource::Named("users".to_string()))
                    .with_badge("12".to_string()),
                SidebarItem::new("orders".to_string(), "Orders")
                    .with_icon(IconSource::Named("shopping-cart".to_string()))
                    .with_badge("3".to_string()),
                SidebarItem::new("settings".to_string(), "Settings")
                    .with_icon(IconSource::Named("settings".to_string())),
            ])
            .selected_id(self.section.clone())
            .variant(SidebarVariant::Fixed)
            .expanded_width(px(230.0));

        let header = HStack::new()
            .items_center()
            .justify_between()
            .px(px(28.0))
            .py(px(16.0))
            .border_b_1()
            .border_color(tokens.border)
            .child(
                VStack::new()
                    .gap(px(2.0))
                    .child(
                        div()
                            .text_size(px(20.0))
                            .font_weight(FontWeight::BOLD)
                            .child("Overview"),
                    )
                    .child(
                        div()
                            .text_size(px(13.0))
                            .text_color(tokens.muted_foreground)
                            .child("Tuesday, June 10 — here's what's happening at Acme"),
                    ),
            )
            .child(
                HStack::new()
                    .gap(px(12.0))
                    .items_center()
                    .child(Input::new(&self.search).size(InputSize::Sm).w(px(280.0)))
                    .child(
                        Button::new("notifications", "")
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Icon)
                            .icon(IconSource::Named("bell".to_string())),
                    )
                    .child(Avatar::new().name("Augustus Otu").size(AvatarSize::Sm)),
            );

        let stats = HStack::new()
            .gap(px(16.0))
            .child(self.stat_card(
                "Total revenue",
                "$128,420",
                "+12.4%",
                true,
                vec![38.0, 42.0, 40.0, 47.0, 52.0, 49.0, 58.0, 64.0],
                &tokens,
            ))
            .child(self.stat_card(
                "Active users",
                "24,310",
                "+8.1%",
                true,
                vec![12.0, 14.0, 15.0, 14.5, 17.0, 19.0, 21.0, 24.0],
                &tokens,
            ))
            .child(self.stat_card(
                "Orders",
                "1,847",
                "+3.6%",
                true,
                vec![5.0, 6.0, 5.5, 7.0, 6.5, 7.5, 8.0, 8.4],
                &tokens,
            ))
            .child(self.stat_card(
                "Churn",
                "1.9%",
                "-0.4%",
                false,
                vec![3.1, 2.9, 2.8, 2.6, 2.4, 2.2, 2.0, 1.9],
                &tokens,
            ));

        let main = VStack::new().flex_1().min_w(px(0.0)).child(header).child(
            div().flex_1().min_h(px(0.0)).child(scrollable_vertical(
                div()
                    .flex()
                    .flex_col()
                    .p(px(28.0))
                    .gap(px(20.0))
                    .child(stats)
                    .child(
                        HStack::new()
                            .gap(px(16.0))
                            .child(self.revenue_chart(&tokens))
                            .child(self.regions_chart()),
                    )
                    .child(
                        Card::new()
                            .header(
                                HStack::new()
                                    .justify_between()
                                    .items_center()
                                    .child(
                                        div()
                                            .text_size(px(15.0))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child("Recent orders"),
                                    )
                                    .child(
                                        Button::new("view-all", "View all")
                                            .variant(ButtonVariant::Outline)
                                            .size(ButtonSize::Sm),
                                    ),
                            )
                            .content(div().w_full().child(self.orders.clone())),
                    ),
            )),
        );

        div()
            .size_full()
            .flex()
            .bg(tokens.background)
            .text_color(tokens.foreground)
            .child(sidebar)
            .child(main)
    }
}

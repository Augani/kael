//! Analytics dashboard template: sidebar navigation, stat cards, charts, and a
//! live data table — everything a browser-runtime dashboard does, fully native.

use kael_ui::components::icon_source::IconSource;
use kael_ui::components::input::{Input, InputSize, InputState};
use kael_ui::components::input_state::InputEvent;
use kael_ui::components::scrollable::scrollable_vertical;
use kael_ui::navigation::sidebar::{Sidebar, SidebarItem, SidebarVariant};
use kael_ui::prelude::*;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

const MAX_ASSET_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ASSET_LIST_ENTRIES: usize = 4096;

#[cfg(debug_assertions)]
actions!(dashboard, [ToggleInspector]);

struct Assets {
    base: PathBuf,
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        let path = self.resolve(path)?;
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Ok(None);
        }
        if metadata.len() > MAX_ASSET_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "asset exceeds the 16 MiB limit",
            )
            .into());
        }
        let mut data = Vec::with_capacity(metadata.len() as usize);
        std::fs::File::open(path)?
            .take(MAX_ASSET_BYTES + 1)
            .read_to_end(&mut data)?;
        if data.len() as u64 > MAX_ASSET_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "asset grew beyond the 16 MiB limit while reading",
            )
            .into());
        }
        Ok(Some(std::borrow::Cow::Owned(data)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let path = self.resolve(path)?;
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Ok(Vec::new());
        }
        std::fs::read_dir(path)?
            .take(MAX_ASSET_LIST_ENTRIES)
            .map(|entry| {
                let name = entry?.file_name().into_string().map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "asset name is not valid UTF-8",
                    )
                })?;
                Ok(SharedString::from(name))
            })
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(Into::into)
    }
}

impl Assets {
    fn resolve(&self, path: &str) -> Result<PathBuf> {
        let relative = Path::new(path);
        if relative.as_os_str().is_empty()
            || relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "asset path must be a non-empty relative path",
            )
            .into());
        }
        Ok(self.base.join(relative))
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

fn filter_orders(query: &str) -> Vec<Order> {
    let query = query.trim().to_lowercase();
    sample_orders()
        .into_iter()
        .filter(|order| {
            query.is_empty()
                || order.customer.to_lowercase().contains(&query)
                || order.product.to_lowercase().contains(&query)
                || format!("#{}", order.id).contains(&query)
        })
        .collect()
}

fn main() -> Result<()> {
    Application::try_new()?
        .with_assets(Assets {
            base: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/kael_ui"),
        })
        .run(|cx| {
            kael_ui::init(cx);
            kael_ui::set_icon_base_path("assets/icons");
            install_theme(cx, Theme::dark());

            #[cfg(debug_assertions)]
            {
                kael_ui::devtools::install_inspector(cx);
                cx.bind_keys([KeyBinding::new("cmd-alt-i", ToggleInspector, None)]);
            }

            if let Err(error) = cx.open_window(
                WindowOptions {
                    titlebar: Some(TitlebarOptions {
                        title: Some("Acme Analytics".into()),
                        ..Default::default()
                    }),
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1380.0), px(900.0)),
                    })),
                    window_min_size: Some(size(px(1040.0), px(700.0))),
                    ..Default::default()
                },
                |_, cx| cx.new(DashboardApp::new),
            ) {
                eprintln!("failed to open the dashboard window: {error}");
                cx.quit();
                return;
            }
            cx.activate(true);
        });
    Ok(())
}

struct DashboardApp {
    search: Entity<InputState>,
    orders: Entity<DataTable<Order>>,
    section: String,
    notifications_open: bool,
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
            let filtered = filter_orders(search.read(cx).content());
            this.orders
                .update(cx, |table, cx| table.set_data(filtered, cx));
        })
        .detach();

        Self {
            search,
            orders,
            section: "dashboard".into(),
            notifications_open: false,
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

    fn section_copy(&self) -> (&'static str, &'static str) {
        match self.section.as_str() {
            "analytics" => ("Analytics", "Revenue and regional performance"),
            "customers" => ("Customers", "Customer activity across recent orders"),
            "orders" => ("Orders", "Search and review recent transactions"),
            "settings" => ("Settings", "Configure this dashboard for your product"),
            _ => ("Overview", "Here's what's happening at Acme"),
        }
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
            .min_w(px(220.0))
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
            .min_w(px(520.0))
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
            .flex_shrink_0()
    }
}

impl Render for DashboardApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = use_theme();
        let tokens = theme.tokens.clone();
        let dashboard = cx.entity();
        let (section_title, section_description) = self.section_copy();

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
            .on_select(move |section, _, cx| {
                let section = section.clone();
                dashboard.update(cx, |dashboard, cx| {
                    dashboard.section = section;
                    cx.notify();
                });
            })
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
                            .child(section_title),
                    )
                    .child(
                        div()
                            .text_size(px(13.0))
                            .text_color(tokens.muted_foreground)
                            .child(section_description),
                    ),
            )
            .child(
                HStack::new()
                    .gap(px(12.0))
                    .items_center()
                    .child(
                        Input::new(&self.search)
                            .aria_label("Search orders and customers")
                            .size(InputSize::Sm)
                            .w(px(280.0)),
                    )
                    .child(
                        Button::new("notifications", "")
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Icon)
                            .tooltip("Notifications")
                            .pressed(self.notifications_open)
                            .on_click(cx.listener(|dashboard, _, _, cx| {
                                dashboard.notifications_open = !dashboard.notifications_open;
                                cx.notify();
                            }))
                            .icon(IconSource::Named("bell".to_string())),
                    )
                    .child(Avatar::new().name("Augustus Otu").size(AvatarSize::Sm)),
            );

        let stats = HStack::new()
            .flex_wrap()
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

        let charts = HStack::new()
            .flex_wrap()
            .gap(px(16.0))
            .child(self.revenue_chart(&tokens))
            .child(self.regions_chart());
        let orders = Card::new()
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
                            .size(ButtonSize::Sm)
                            .on_click(cx.listener(|dashboard, _, _, cx| {
                                dashboard.section = "orders".to_string();
                                cx.notify();
                            })),
                    ),
            )
            .content(div().w_full().child(self.orders.clone()));
        let notifications = Card::new().content(
            HStack::new()
                .items_center()
                .justify_between()
                .child("You're all caught up — no new notifications.")
                .child(
                    Button::new("dismiss-notifications", "Dismiss")
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .on_click(cx.listener(|dashboard, _, _, cx| {
                            dashboard.notifications_open = false;
                            cx.notify();
                        })),
                ),
        );
        let settings = Card::new()
            .header(
                div()
                    .text_size(px(15.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Dashboard settings"),
            )
            .content(
                VStack::new()
                    .gap(px(8.0))
                    .child("This template keeps settings intentionally read-only.")
                    .child(
                        div()
                            .text_size(px(13.0))
                            .text_color(tokens.muted_foreground)
                            .child("Connect these controls to your application's persisted preferences."),
                    ),
            );
        let show_stats = matches!(self.section.as_str(), "dashboard" | "analytics");
        let show_charts = matches!(self.section.as_str(), "dashboard" | "analytics");
        let show_orders = matches!(self.section.as_str(), "dashboard" | "customers" | "orders");

        let content = div()
            .flex()
            .flex_col()
            .p(px(28.0))
            .gap(px(20.0))
            .when(self.notifications_open, |content| {
                content.child(notifications)
            })
            .when(show_stats, |content| content.child(stats))
            .when(show_charts, |content| content.child(charts))
            .when(show_orders, |content| content.child(orders))
            .when(self.section == "settings", |content| {
                content.child(settings)
            });

        let main = VStack::new().flex_1().min_w(px(0.0)).child(header).child(
            div()
                .flex_1()
                .min_h(px(0.0))
                .child(scrollable_vertical(content)),
        );

        let root = div()
            .size_full()
            .flex()
            .bg(tokens.background)
            .text_color(tokens.foreground)
            .child(sidebar)
            .child(main);

        #[cfg(debug_assertions)]
        let root = root.on_action(|_: &ToggleInspector, window, cx| {
            window.toggle_inspector(cx);
        });

        root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_search_is_case_insensitive_and_supports_ids() {
        assert_eq!(filter_orders("ada").len(), 1);
        assert_eq!(filter_orders("ENTERPRISE").len(), 2);
        assert_eq!(filter_orders("#1040").len(), 1);
        assert_eq!(filter_orders("   ").len(), sample_orders().len());
    }

    #[test]
    fn asset_paths_are_confined_to_the_asset_root() {
        let assets = Assets {
            base: PathBuf::from("/tmp/assets"),
        };
        assert!(assets.resolve("icons/bell.svg").is_ok());
        assert!(assets.resolve("../secret").is_err());
        assert!(assets.resolve("/etc/passwd").is_err());
    }
}

//! Read-only records browser with optional application-owned compact text.
use kael_ui::prelude::*;
use smol_str::SmolStr;
use std::{cell::RefCell, rc::Rc, sync::Arc};

kael::actions!(compact_records_example, [QuitRecords]);
const ROWS: usize = 100_000;
const PAGE_ROWS: usize = 64;
const HEADERS: [&str; 8] = [
    "Record", "Team", "Status", "Score", "Sprint", "Date", "Tags", "Notes",
];
const COLUMN_WIDTHS: [f32; 8] = [200.0, 120.0, 104.0, 80.0, 104.0, 144.0, 112.0, 200.0];

struct Record {
    id: u64,
    fields: [SmolStr; 8],
}
struct Store {
    records: Arc<[Arc<Record>]>,
}
impl Store {
    fn build() -> Arc<Self> {
        let records = (0..ROWS)
            .map(|row| {
                let mut fields = std::array::from_fn(|column| {
                    SmolStr::from(match column {
                        0 => format!("Record {row:06} 日本語"),
                        1 => format!("Team {:02} café", row % 97),
                        2 => format!("{} · ✓", ["Ready", "Review", "Active"][row % 3]),
                        3 => format!("{:03} 点", row.wrapping_mul(37) % 1000),
                        4 => format!("Sprint {:02} Δ", row / 1000 % 52),
                        5 => format!("2026-10-{:02} 火", row % 28 + 1),
                        6 => format!("tag{:02} 👩‍💻", row % 31),
                        _ => format!("Note {row:06} · naïve"),
                    })
                });
                if row == 17 {
                    // A long existing Arc stays shared through SmolStr and the UI adapter.
                    let note: Arc<str> =
                        "日本語 👩🏽‍💻 café — a longer record note. ".repeat(128).into();
                    fields[7] = SmolStr::from(note);
                }
                Arc::new(Record {
                    id: 1_000_003 + row as u64 * 17,
                    fields,
                })
            })
            .collect::<Vec<_>>()
            .into();
        Arc::new(Self { records })
    }
}
struct Query {
    store: Arc<Store>,
    reversed: bool,
}
impl Query {
    fn row(&self, position: usize) -> Arc<Record> {
        let index = if self.reversed {
            self.store.records.len() - 1 - position
        } else {
            position
        };
        self.store.records[index].clone()
    }
    fn page(&self, request: DataTablePageRequest) -> Vec<Arc<Record>> {
        (request.page_start()..request.page_start() + request.page_size())
            .map(|position| self.row(position))
            .collect()
    }
}
struct RecordsBrowser {
    table: Option<Entity<DataTable<Arc<Record>>>>,
    source: Option<Rc<RefCell<Query>>>,
    // Retain the completed task handle for the view lifetime; do not cancel
    // its currently executing future from inside the install callback.
    _build_task: Option<Task<()>>,
    reversed: bool,
    status: String,
}
impl RecordsBrowser {
    fn new(cx: &mut Context<Self>) -> Self {
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |view, cx| {
            let store = executor.spawn(async { Store::build() }).await;
            let _ = view.update(cx, |view, cx| view.install(store, cx));
        });
        Self {
            table: None,
            source: None,
            _build_task: Some(task),
            reversed: false,
            status: "Loading records…".into(),
        }
    }
    fn install(&mut self, store: Arc<Store>, cx: &mut Context<Self>) {
        let source = Rc::new(RefCell::new(Query {
            store,
            reversed: false,
        }));
        let fetch_source = source.clone();
        let selected = cx.entity().downgrade();
        let table = cx.new(|cx| {
            let columns = HEADERS
                .into_iter()
                .enumerate()
                .map(|(column, header)| {
                    ColumnDef::new(
                        SharedString::new(format!("column-{column}")),
                        SharedString::new_static(header),
                        move |row: &Arc<Record>| {
                            let arc: Arc<str> = row.fields[column].clone().into();
                            SharedString::new(arc)
                        },
                    )
                    .width(px(COLUMN_WIDTHS[column]))
                    .resizable(false)
                    .sortable(false)
                    .editable(false)
                })
                .collect();
            DataTable::new_virtual(ROWS, columns, PAGE_ROWS, cx)
                .show_search(false)
                .fill_parent(true)
                .max_cached_pages(16)
                .max_in_flight_pages(4)
                .on_fetch_page_request(move |request, _, cx| {
                    // These in-memory source reads are bounded to 64 Arc clones.
                    // Completion is deferred so query-generation rejection remains real.
                    let rows = fetch_source.borrow().page(request);
                    let table = cx.entity().downgrade();
                    cx.defer(move |cx| {
                        let _ = table.update(cx, |table, cx| {
                            table.set_page_data_for(request, rows, cx);
                        });
                    });
                })
                .on_row_click(move |_, record, _, cx| {
                    let id = record.id; // Actual stored ID, not the displayed position.
                    let _ = selected.update(cx, |view, cx| {
                        view.status = format!("Selected record {id}.");
                        cx.notify();
                    });
                })
        });
        self.table = Some(table);
        self.source = Some(source);
        self.status = "Records loaded. Select a row to see its record ID.".into();
        cx.notify();
    }
    fn reload(&mut self, reverse: bool, cx: &mut Context<Self>) {
        let (Some(source), Some(table)) = (&self.source, &self.table) else {
            return;
        };
        if reverse {
            self.reversed = !self.reversed;
        }
        source.borrow_mut().reversed = self.reversed;
        // Reset both cache and positional selection, and retire old page requests.
        table.update(cx, |table, cx| {
            table.virtual_reset(ROWS, Some(PAGE_ROWS), cx)
        });
        self.status = "Records loaded. Select a row to see its record ID.".into();
        cx.notify();
    }
}
impl Render for RecordsBrowser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let reverse = cx.entity().downgrade();
        let refresh = reverse.clone();
        let loading = self.table.is_none();
        let theme = Theme::of(cx);
        div()
            .size_full()
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .bg(theme.tokens.background)
            .text_color(theme.tokens.foreground)
            .child(
                div()
                    .flex()
                    .items_center()
                    .flex_shrink_0()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Records · 100,000"),
                    )
                    .child(
                        Button::new("records-refresh", "Refresh")
                            .disabled(loading)
                            .on_click(move |_, _, cx| {
                                let _ = refresh.update(cx, |view, cx| view.reload(false, cx));
                            }),
                    )
                    .child(
                        Button::new(
                            "records-order",
                            if self.reversed {
                                "Ascending"
                            } else {
                                "Descending"
                            },
                        )
                        .disabled(loading)
                        .on_click(move |_, _, cx| {
                            let _ = reverse.update(cx, |view, cx| view.reload(true, cx));
                        }),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .flex_shrink_0()
                    .text_color(theme.tokens.muted_foreground)
                    .child("Browse the records or change their order. This view is read-only."),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .when_some(self.table.as_ref(), |element, table| {
                        element.child(table.clone())
                    }),
            )
            .child(div().text_sm().flex_shrink_0().child(self.status.clone()))
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    Application::try_new()?.run(|cx| {
        kael_ui::init(cx);
        install_theme(cx, Theme::astryx_neutral());
        cx.on_action(|_: &QuitRecords, cx| cx.quit());
        cx.bind_keys([
            KeyBinding::new("cmd-q", QuitRecords, None),
            KeyBinding::new("ctrl-q", QuitRecords, None),
        ]);
        cx.set_menus(
            StandardMacMenuBar::new("Kael records")
                .file_menu(file_menu().action("Quit Kael records", QuitRecords))
                .build(),
        );
        let bounds = Bounds::centered(None, size(px(1100.0), px(760.0)), cx);
        if let Err(error) = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("Kael records");
                cx.new(RecordsBrowser::new)
            },
        ) {
            eprintln!("failed to open records: {error}");
            cx.quit();
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reverse_looks_up_stored_ids_without_treating_positions_as_ids() {
        let store = Arc::new(Store {
            records: [42, 9_007, 501]
                .into_iter()
                .map(|id| {
                    Arc::new(Record {
                        id,
                        fields: std::array::from_fn(|_| SmolStr::new("value")),
                    })
                })
                .collect::<Vec<_>>()
                .into(),
        });
        let ascending = Query {
            store: store.clone(),
            reversed: false,
        };
        let descending = Query {
            store,
            reversed: true,
        };
        assert_eq!(
            [
                ascending.row(0).id,
                ascending.row(1).id,
                ascending.row(2).id
            ],
            [42, 9_007, 501]
        );
        assert_eq!(
            [
                descending.row(0).id,
                descending.row(1).id,
                descending.row(2).id
            ],
            [501, 9_007, 42]
        );
        assert!(Arc::ptr_eq(&ascending.row(0), &descending.row(2)));
    }
    #[test]
    fn long_business_text_adapter_shares_existing_heap_arc() {
        let input: Arc<str> = "日本語 👩🏽‍💻 café\r\n".repeat(1024).into();
        let field = SmolStr::from(input.clone());
        let ui = SharedString::new(Arc::<str>::from(field.clone()));
        assert_eq!(ui.as_ref(), input.as_ref());
        let returned: Arc<str> = ui.into();
        assert!(Arc::ptr_eq(&input, &returned));
    }
}

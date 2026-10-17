# Lists & Data

High-performance list components with virtualization for rendering thousands of items.

---

## UniformList

Highest-performance list for items of equal, positive height. It measures one row and only renders
the visible range, so large logs and tables do not create an element for every record:

```rust
use kael::{uniform_list, UniformListScrollHandle};

let scroll_handle = UniformListScrollHandle::new();

uniform_list(
    "log-entries",
    self.entries.len(),
    {
        let entries = self.entries.clone();
        move |range, _window, _cx| {
            entries[range.clone()]
                .iter()
                .map(|entry| {
                    div()
                        .px_3()
                        .py_1()
                        .text_sm()
                        .child(entry.message.clone())
                        .into_any_element()
                })
                .collect()
        }
    },
)
.track_scroll(scroll_handle.clone())
```

**When to use:** Log viewers, file lists, data tables — any list where every row has the same height.

The measured row must resolve to a finite height greater than zero. A zero, negative, NaN, or
infinite height is treated as an empty viewport for that frame instead of attempting an invalid
visible-range calculation. Use `with_width_from_item(Some(index))` when a representative row is a
better width sample than row zero.

---

## List

Flexible list with alignment and overflow handling:

```rust
use kael::list;

// Basic list
list()
    .child(div().child("Item 1"))
    .child(div().child("Item 2"))
    .child(div().child("Item 3"))
```

---

## RecyclingList

Virtualized list for items with different heights. Supply a delegate with stable estimated heights
for rows that have not been measured yet:

```rust
use kael::{
    AnyElement, App, FontWeight, IntoElement, ListDelegate, Pixels, Window, div, px,
    recycling_list,
};
use std::sync::Arc;

#[derive(Clone)]
struct MessageDelegate {
    messages: Arc<[Message]>,
    // Increment when messages are inserted, removed, reordered, or their
    // estimated heights change.
    height_revision: u64,
}

impl ListDelegate for MessageDelegate {
    fn item_count(&self) -> usize {
        self.messages.len()
    }

    fn estimated_item_height(&self, index: usize) -> Pixels {
        let body_lines = self.messages[index].body.lines().count().max(1);
        px(44.0 + body_lines as f32 * 18.0)
    }

    fn estimated_heights_revision(&self) -> Option<u64> {
        Some(self.height_revision)
    }

    fn render_item(&self, index: usize, _window: &mut Window, _cx: &mut App) -> AnyElement {
        let msg = &self.messages[index];
        div()
            .p_3()
            .child(div().font_weight(FontWeight::BOLD).child(msg.sender.clone()))
            .child(div().text_sm().child(msg.body.clone()))
            .into_any_element()
    }
}

recycling_list(
    "messages",
    MessageDelegate {
        messages: self.messages.clone(),
        height_revision: self.message_height_revision,
    },
)
```

**When to use:** Chat messages, feed items — lists where rows vary in height.

Returning `Some(revision)` from `estimated_heights_revision` is the fast path: unchanged frames
reuse the existing height sum-tree without an O(total items) estimation pass. Increment the
revision whenever count, order, or estimates change. The default return value is `None`; that is
safe for fully dynamic delegates because estimates are refreshed every frame, but it intentionally
trades away the steady-state optimization.

Element pooling is opt-in through `recycle_key` and `render_recycled_item`. Kael sizes each keyed
pool from the observed visible-and-overdraw high-water mark, so large viewports are not constrained
by a small fixed pool. Only return elements whose `supports_reuse()` implementation is true, and
fully update recycled content before returning it for a new index.

---

## SortableList

Drag-to-reorder list with auto-scroll and insertion indicator:

```rust
use kael::sortable_list;

sortable_list(
    "layers",
    self.layers.len(),
    {
        let layers = self.layers.clone();
        move |index, _window, _cx| {
            div()
                .px_3()
                .py_2()
                .child(layers[index].name.clone())
                .into_any_element()
        }
    },
)
.on_reorder({
    let entity = entity.clone();
    move |from, to, _window, cx| {
        entity.update(cx, |this, cx| {
            let item = this.layers.remove(from);
            this.layers.insert(to, item);
            cx.notify();
        });
    }
})
```

**When to use:** Layer panels, playlist editors, kanban columns — anywhere users reorder items by dragging.

---

## ScrollBar

Kael provides automatic scrollbars for any element with `overflow_y_scroll()`
or `overflow_y_auto()` and a tracked `ScrollHandle`. The scrollbar appears
as a native-style dark rounded thumb when content overflows — no extra code
needed (see [Layout & Styling](layout-and-styling.md#overflow-and-scrolling)).

For **custom** scroll bar rendering, use the explicit `scroll_bar()` widget:

```rust
use kael::scroll_bar;

scroll_bar(scroll_handle.clone())
    .render_with(|state, bounds, window, _cx| {
        // Custom scroll bar rendering
        // state.thumb_bounds, state.dragging
    })
```

---

## Patterns

### Virtual `DataTable` selection and paging

`DataTable::new_virtual` keeps only a bounded LRU of pages and virtualizes both
rows and variable-width columns. A million-row select-all is represented as
"all except these deselected rows", so it stores zero row indices until a user
starts excluding rows:

```rust,ignore
DataTable::new_virtual(1_000_000, columns, 128, cx)
    .max_cached_pages(8)
    .max_in_flight_pages(4)
    .show_selection(true)
    .on_fetch_page_request(|request, _window, cx| {
        // Fetch or compute only request.page_start()..page_start + page_size.
        // Commit with set_page_data_for(request, rows, cx); stale generations
        // are rejected automatically. Report errors with page_load_failed.
    })
    .on_selection_change_snapshot(|selection, _window, _cx| {
        println!(
            "{} selected; {} stored indices ({})",
            selection.selected_count(),
            selection.stored_index_count(),
            selection.representation_key(),
        );
    })
```

Use `on_selection_change_snapshot` for bulk actions. Its
`DataTableSelectionSnapshot::AllExcept { total_rows, deselected }` variant is
exact without expansion. The compatibility `on_selection_change(&[usize], ...)`
callback is invoked only when the exact selected indices can be materialized
within 16,384 items; it is intentionally skipped for a million-row all-except
selection rather than reporting an empty or partial slice. Likewise,
`selected_rows()` returns `None` for all-except state; this does not mean the
selection is empty.

Virtual-table search and sort are query inputs for the backing source. Kael
invalidates the current generation and cache; the application or a Kael Web
Worker performs the large search/sort and returns the requested page. Kael does
not scan or allocate the million-row logical range on the UI thread.

Virtual paging admits at most eight pending pages per query by default;
`max_in_flight_pages` configures this limit with a minimum of one. Finish each
request with `set_page_data_for` (or legacy `set_page_data`) and report errors
with `page_load_failed` to free its slot.
When a slot opens, the next frame considers the current viewport without
retaining a queue of previously visited pages. Cached pages use a separate
`max_cached_pages` limit.

Fetch tasks belong to the application. Reset, search, and sort invalidate the
query and its reservations, but do not cancel tasks from older queries. Cancel
those tasks in the data source when a limit across queries is required. Prefer
the generation-safe callback above for asynchronous sources; legacy
`on_fetch_page`/`set_page_data` callbacks cannot identify stale query results.

### Keep business records independent from UI text

`DataTable<T>` can retain an application's own record type instead of converting
an entire dataset to UI strings. The `compact_records` example uses
`DataTable<Arc<Record>>` with the optional application dependency
`smol_str = "=0.3.6"`:

```sh
cargo run --release -p kael_ui --no-default-features --features native --example compact_records
```

It prepares 100,000 records on a worker, retaining eight compact fields and an
explicit stable ID per record. The table receives at most 64 row Arcs per page,
keeps at most 16 pages, and admits four pending pages in the current query. Column
accessors convert only mounted cell values to the existing UI string:

```rust,ignore
let arc: std::sync::Arc<str> = record.fields[column].clone().into();
SharedString::new(arc)
```

Short `SmolStr` values can stay inline in business records, then acquire owned Arc
storage when displayed. Long heap values share their existing Arc through this
adapter. Converting static application text through `SmolStr` into an Arc can
still allocate; use `SharedString::new_static` directly for known static UI
headers. Other applications can use typed fields, database records or ordinary
strings. Kael's public `SharedString`, `ArcCow` and conversion APIs remain unchanged.

The example is read-only: columns are explicitly non-editable, and built-in
search/sort controls are disabled in favor of the application's ordering controls.
Selecting a row reads its stored ID from the record rather than interpreting its
displayed position as an ID. Refresh and order changes reset the virtual query;
page completion passes the original `DataTablePageRequest` to
`set_page_data_for`, which rejects stale generations.

The in-memory source clones only the requested row Arcs and defers completion.
A network/database source must additionally cancel its own old tasks: the table's
pending-page limit covers the current generation rather than all historical work.
The example's initial model construction runs synchronously on a worker until it
returns; it does not offer preemptible cancellation of that construction loop.

The related retained-payload measurement covers two immutable 100,000x8 fixtures
and actual `RemoteSheetState<Query>` / `VirtualSheetGrid` tile and accessibility
storage. This example instead retains one store with explicit IDs and per-record
Arcs, using the read-only `DataTable` control. Those results do not measure this
example's exact footprint, native presentation or throughput, and are not a
universal memory ratio. The matched desktop comparison's model is unchanged.

#### Writes and query identity

For writable sources, `RemoteSheetEvent::Edited` captures the query/generation at
edit time. Resolve the original business ID from that captured query if a later
query changes displayed positions. Generation checks prevent an old tile response
from replacing current data; they do not implement application persistence.

A writer needs a bounded admission and recovery policy. Retain failed values and
their stable IDs while retries remain possible, or prevent query changes from
retiring pending edits. A failed-ID set or error message does not preserve the
failed value. Do not promise Undo recovery after the original query's positional
history has already been retired. A bounded write queue also needs explicit
backpressure before it accepts more values.

`RemoteSheetState::set_query` retires positional edits/history and semantic IDs;
`refresh` retries the same records while preserving local history. Different
logical dimensions require constructing a new grid/controller. Successful-write,
query-identity and cancellation checks do not prove a failed-persistence recovery
workflow. The read-only example implements no writable-cell or persistence-queue
promise.

### Data table with uniform_list

```rust
struct DataTable {
    rows: Vec<Row>,
    columns: Vec<Column>,
    scroll: UniformListScrollHandle,
}

impl Render for DataTable {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let columns = self.columns.clone();
        let rows = self.rows.clone();

        div().flex().flex_col().size_full()
            .child(self.render_header())
            .child(
                uniform_list("table-body", rows.len(), move |range, _w, _cx| {
                    rows[range.clone()].iter().map(|row| {
                        div().flex().flex_row()
                            .children(columns.iter().map(|col| {
                                div().w(px(col.width)).px_2().py_1()
                                    .child(row.get(&col.key).clone())
                            }))
                            .into_any_element()
                    }).collect()
                })
                .track_scroll(self.scroll.clone())
            )
    }
}
```

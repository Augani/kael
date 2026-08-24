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
    .show_selection(true)
    .on_fetch_page_request(|request, _window, cx| {
        // Fetch or compute only request.page_start()..page_start + page_size.
        // Commit with set_page_data_for(request, rows, cx); stale generations
        // are rejected automatically.
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

# Lists & Data

High-performance list components with virtualization for rendering thousands of items.

---

## UniformList

Highest-performance list for items of equal height. Only renders visible items — handles 100K+ items smoothly:

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

Virtualized list for items with different heights. Recycles DOM nodes for performance:

```rust
use kael::recycling_list;

recycling_list(
    "messages",
    self.messages.len(),
    move |index, _window, _cx| {
        let msg = &messages[index];
        div()
            .p_3()
            .child(div().font_weight(FontWeight::BOLD).child(msg.sender.clone()))
            .child(div().text_sm().child(msg.body.clone()))
            .into_any_element()
    },
)
```

**When to use:** Chat messages, feed items — lists where rows vary in height.

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

Custom scroll bar bound to a scroll handle:

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

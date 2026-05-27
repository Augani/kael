# Layout & Styling

Kael uses GPU-accelerated flexbox (powered by [Taffy](https://github.com/DioxusLabs/taffy)) with a Tailwind-inspired API. Every style is a method call on a `Div`.

## Flexbox layout

```rust
div().flex().flex_row().gap_2()
    .child(div().child("Left"))
    .child(div().child("Right"))

div().flex().flex_col().gap_4()
    .child(div().child("Top"))
    .child(div().child("Bottom"))
```

### Alignment

```rust
div().flex()
    .items_center()
    .justify_center()
    .justify_between()
    .items_start()
    .items_end()
```

### Flex sizing

```rust
div().flex_1()
div().flex_grow()
div().flex_shrink_0()
div().flex_none()
```

## Grid layout

Switch a container to CSS Grid with `.grid()`, define tracks with `.grid_cols(n)` / `.grid_rows(n)`, and place children with `.col_span(n)` / `.row_span(n)` (or `.col_span_full()` / `.row_span_full()`). `.gap_*()` sets the gutters:

```rust
div()
    .grid()
    .grid_cols(5)
    .grid_rows(5)
    .gap_1()
    .child(div().row_span(1).col_span_full().child("Header"))
    .child(div().col_span(1).row_span(3).child("Sidebar"))
    .child(div().col_span(3).row_span(3).child("Content"))
    .child(div().col_span(1).row_span(3).child("Aside"))
    .child(div().row_span(1).col_span_full().child("Footer"))
```

See `examples/grid_layout.rs` for a full "holy grail" layout.

## Sizing

```rust
div().w(px(200.0)).h(px(100.0))

div().w_full()
div().h_full()
div().size_full()

div().size_8()
div().w_12()
div().h_6()

div().min_w(px(200.0)).max_w(px(600.0))
```

## Spacing

```rust
div().p_4()
div().px_3()
div().py_2()
div().pt_1()
div().pl(px(20.0))

div().m_4()
div().mx_auto()
div().mt_2()

div().flex().gap_2()
div().flex().gap_4()
```

## Colors

```rust
div().bg(rgb(0x1E1E1E))
div().text_color(rgb(0xFFFFFF))
div().border_color(rgb(0x3C3C3C))

div().bg(rgba(0x00000080))

div().bg(kael::red())
div().bg(kael::blue())
div().bg(kael::white())
div().bg(kael::black())

use kael::hsla;
div().bg(hsla(210.0 / 360.0, 1.0, 0.5, 1.0))
```

## Borders

```rust
div().border_1()
div().border_2()
div().border_t_1()
div().border_b_1()
div().border_l_1()
div().border_r_1()
div().border_color(rgb(0x3C3C3C))
div().border_dashed()
```

## Corners

```rust
div().rounded_sm()
div().rounded_md()
div().rounded_lg()
div().rounded_full()
div().rounded(px(8.0))
```

By default, rounded corners use continuous (squircle) rounding to match SwiftUI's
`RoundedRectangle` shape on macOS. Use `.circular_corners()` to opt into the
legacy pure quarter-circle look:

```rust
div().rounded(px(8.0)).circular_corners()
```

## Shadows

```rust
div().shadow_sm()
div().shadow_md()
div().shadow_lg()
div().shadow_xl()
```

## Typography

```rust
div()
    .text_xs()
    .text_sm()
    .text_base()
    .text_lg()
    .text_xl()
    .text_2xl()
    .text_3xl()

div().font_weight(FontWeight::BOLD)
div().font_family(".SystemUIFont")
```

## Overflow and scrolling

Control how content behaves when it exceeds the element bounds:

```rust
div().overflow_hidden()
div().overflow_x_scroll()
div().overflow_y_scroll()
div().overflow_y_auto()
    .id("scroll-container")
```

When using `overflow_y_scroll()` or `overflow_y_auto()` with a `ScrollHandle`,
Kael automatically renders a macOS-style scrollbar thumb when content overflows.
No extra widget is needed:

```rust
let scroll_handle = ScrollHandle::new();

div()
    .id("my-scrollable")
    .overflow_y_scroll()
    .track_scroll(&scroll_handle)
    .child(long_content)
```

The auto-scrollbar appears only when content exceeds the viewport and tracks
the scroll position automatically. To keep scrollbars visible at all times
(instead of auto-hiding after idle):

```rust
let scroll_handle = ScrollHandle::new().always_show_scrollbars();
```

For custom scrollbar styling, use the explicit `scroll_bar()` widget instead
(see [Lists & Data](lists-and-data.md)).

## Positioning

```rust
div().relative()
    .child(
        div().absolute()
            .top(px(10.0))
            .right(px(10.0))
            .child("Badge")
    )
```

## Opacity

```rust
div().opacity(0.5)
```

## Cursor

```rust
div().cursor_pointer()
div().cursor_default()
```

## Conditional styling with `.when()`

```rust
div()
    .when(self.is_selected, |this| {
        this.bg(rgb(0x2563eb)).text_color(rgb(0xffffff))
    })
    .when(!self.is_selected, |this| {
        this.bg(rgb(0xffffff)).text_color(rgb(0x000000))
    })
```

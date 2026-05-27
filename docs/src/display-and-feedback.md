# Display & Feedback

Elements for showing information and providing feedback to users.

---

## Text

Basic text rendering. Strings passed to `.child()` automatically become text elements:

```rust
div().child("Hello, world!")

div().text_xl().text_color(rgb(0x2563eb)).child("Title")
```

For styled inline text, use `SharedString`:

```rust
use kael::SharedString;

let label: SharedString = "Click me".into();
div().child(label)
```

---

## Label

Accessible label that forwards focus to a target control:

```rust
use kael::label;

label("Email address", "email-input")
// Clicking the label focuses the text_input with id "email-input"
```

---

## Icon

Render named icons from the icon set:

```rust
use kael::icon;

icon("folder")
icon("file").size(px(16.0))
```

---

## Image

Display raster images with caching:

```rust
use kael::{img, ImageSource};

img(ImageSource::from_path("photo.png"))
    .w(px(200.0))
    .h(px(150.0))
    .rounded_md()
```

---

## SVG

Render SVG content:

```rust
use kael::svg;

svg()
    .path("icons/logo.svg")
    .w(px(24.0))
    .h(px(24.0))
    .text_color(rgb(0x2563eb))  // fills SVG with color
```

---

## RichText

Compose text from inline styled spans, clickable entities (links, mentions, hashtags), inline code, and embedded elements:

```rust
use kael::{rich_text, FontWeight, HighlightStyle, rgb};

rich_text()
    .text("The ")
    .styled("quick brown fox", HighlightStyle {
        color: Some(rgb(0xb45309).into()),
        font_weight: Some(FontWeight::BOLD),
        ..Default::default()
    })
    .text(" jumps. See the ")
    .link("docs", "https://augani.github.io/kael/", |_window, _app| {})
    .text(" or ping ")
    .mention("@team", "team-id", |_window, _app| {})
    .text(". Run ")
    .code("cargo run")
    .selectable(true)
```

Builder methods: `.text()`, `.styled(text, HighlightStyle)`, `.link(text, target, on_click)`, `.mention(text, payload, on_click)`, `.hashtag(text, payload, on_click)`, `.code(text)`, `.inline_element(element)`, `.selectable(bool)`, `.selection_color(color)`. Entity click handlers have the signature `Fn(&mut Window, &mut App)`.

---

## Progress

Determinate or indeterminate progress indicator:

```rust
use kael::progress;

// Determinate (0.0 to 1.0)
progress("export", 0.65)

// Indeterminate
progress("loading", 0.0).indeterminate()

// Custom rendering
progress("download", self.progress)
    .render_with(|state, bounds, window, _cx| {
        // state.percentage: Option<f64>
        // state.indeterminate: bool
        // Paint track and fill bar using window.paint_quad()
        window.paint_quad(fill(bounds, rgb(0xe2e8f0)).corner_radii(px(4.0)));
        if let Some(pct) = state.percentage {
            let width = bounds.size.width * pct as f32;
            window.paint_quad(fill(
                Bounds::new(bounds.origin, size(width, bounds.size.height)),
                rgb(0x2563eb),
            ).corner_radii(px(4.0)));
        }
    })
```

**ProgressRenderState fields:** `value`, `max`, `percentage`, `indeterminate`

---

## Toast

Auto-dismissing notification overlay:

```rust
use kael::{Toast, ToastStack};

// In your view, create a ToastStack entity
struct MyApp {
    toasts: Entity<ToastStack>,
}

// Create it
let toasts = cx.new(|_| ToastStack::new());

// Push a toast from anywhere with the entity handle
toasts.update(cx, |stack, cx| {
    stack.push(
        Toast::new("File saved")
            .body("changes written to disk")
            .duration(Duration::from_secs(3)),
        window,
        cx,
    );
});

// Render the stack in your view
impl Render for MyApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(/* your content */)
            .child(self.toasts.clone()) // ToastStack renders as overlay
    }
}
```

**Toast positions:** `ToastPosition::TopRight`, `TopCenter`, `BottomRight`

---

## Canvas

GPU-accelerated custom drawing surface:

```rust
use kael::canvas;

canvas(|bounds, window, cx| {
    // Custom painting with window.paint_quad(), window.paint_path(), etc.
    window.paint_quad(fill(bounds, rgb(0x1E1E1E)));
})
.w(px(400.0))
.h(px(300.0))
```

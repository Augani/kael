# Core Concepts

## Application lifecycle

Every Kael app follows this flow:

```
Application::try_new()? → run() → cx.open_window() → cx.new(|_| View) → render loop
```

```rust,ignore
fn main() -> Result<(), Box<dyn std::error::Error>> {
    Application::try_new()?.run(|cx: &mut App| {
        if let Err(error) = cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| MyView {})
        }) {
            eprintln!("failed to open the application window: {error}");
            cx.quit();
        }
    });
    Ok(())
}
```

## Entity\<T\> — reactive state containers

An `Entity<T>` is a handle to a value stored in the framework's arena. When the value changes and you call `cx.notify()`, any view rendering that entity re-renders automatically.

```rust
struct AppState {
    user: String,
    count: i32,
}

let state: Entity<AppState> = cx.new(|_cx| AppState {
    user: "Alice".into(),
    count: 0,
});

let name = state.read(cx).user.clone();

state.update(cx, |this, cx| {
    this.count += 1;
    cx.notify();
});
```

### Entity vs. direct state

If your view struct holds state directly (like `struct Counter { count: i32 }`), the view IS the entity — `cx.new()` wraps it in `Entity<Counter>` automatically. Use separate entities when you need shared state across views:

```rust
struct Sidebar {
    shared: Entity<AppState>,
}

struct Editor {
    shared: Entity<AppState>,
}
```

## The Render trait

Any type that implements `Render` can be displayed in a window:

```rust
impl Render for MyView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().child("Hello")
    }
}
```

**Parameters:**
- `&mut self` — mutable access to your state
- `window: &mut Window` — the window being rendered into (for window-level APIs)
- `cx: &mut Context<Self>` — entity-scoped context for creating entities, subscribing to events, and notifying changes

**Return:** Anything implementing `IntoElement` — a `Div`, a `Button`, or any widget.

## Context types

| Context | Where you get it | What it does |
|---------|-----------------|--------------|
| `App` | `Application::try_new()?.run(\|cx\| { ... })` | Root context — open windows, set globals |
| `Context<T>` | `impl Render` and `cx.new()` closures | Entity-scoped — notify, observe, subscribe |
| `Window` | `impl Render` render method | Window-level — bounds, focus, painting |
| `AsyncApp`, `AsyncWindowContext` | Convert a live context before spawning async work | Fallible access that can safely outlive a window or entity callback |
| `TestAppContext` | Headless framework tests | Deterministic entity, input, and rendering test access |

### Getting an entity handle inside render

```rust
impl Render for MyView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();

        button("click-me")
            .label("Click")
            .on_click(move |_, _, cx| {
                entity.update(cx, |this, cx| {
                    this.handle_click();
                    cx.notify();
                });
            })
    }
}
```

## Global state

For app-wide values (theme, user session, config), use the `Global` trait:

```rust
struct AppConfig {
    dark_mode: bool,
    font_size: f32,
}

impl Global for AppConfig {}

cx.set_global(AppConfig { dark_mode: true, font_size: 14.0 });

cx.read_global::<AppConfig, _>(|config, _| {
    config.dark_mode
});

cx.update_global::<AppConfig, _>(|config, cx| {
    config.dark_mode = false;
});
```

## Element composition

Views compose by nesting elements with `.child()`:

```rust
fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .child(self.render_header())
        .child(self.render_content())
        .child(self.render_footer())
}

fn render_header(&self) -> impl IntoElement {
    div().h(px(48.0)).bg(rgb(0x2563eb)).child("Header")
}
```

### Conditional rendering

Use `.when()` for conditional styling or `.map()` for conditional children:

```rust
div()
    .when(self.is_active, |div| div.bg(rgb(0x2563eb)))
    .when(!self.is_active, |div| div.bg(rgb(0x64748b)))
    .child(if self.show_label { "Active" } else { "Inactive" })
```

### Iterating children

Use `.children()` with an iterator:

```rust
div()
    .flex()
    .flex_col()
    .children(self.items.iter().map(|item| {
        div().px_2().py_1().child(item.name.clone())
    }))
```

## Event handling

All events pass `(event_data, &mut Window, &mut App)`:

```rust
div()
    .id("my-element")
    .on_click(|event, window, cx| {
    })
    .on_mouse_down(MouseButton::Left, |event, window, cx| {
    })
    .on_key_down(|event, window, cx| {
    })
```

Widget events use the same pattern:

```rust
text_input("name", self.name.clone())
    .on_change(|new_value, window, cx| {
    })
    .on_submit(|value, window, cx| {
    })
```

## Subscriptions and observations

Watch for changes on other entities:

```rust
cx.observe(&other_entity, |this, other, cx| {
    cx.notify();
});

cx.subscribe(&other_entity, |this, _other, event: &MyEvent, cx| {
});
```

# Actions & Keybindings

Kael separates *what* happens (an **action**) from *how* it's triggered (a **keybinding** or click). Actions are dispatched up the focused element tree, so a keystroke is routed to the nearest handler in the currently focused context — the same model that powers editor-grade keyboard UX.

## Defining actions

The `actions!` macro generates zero-field action types in a namespace:

```rust
use kael::actions;

actions!(editor, [Save, Undo, Redo, Tab, TabPrev]);
```

Each entry becomes a type (`Save`, `Undo`, …) implementing the `Action` trait, with a stable name like `editor::Save` used for keymaps and dispatch.

## Binding keys

Register bindings once at startup with `cx.bind_keys`. `KeyBinding::new` takes the keystroke string, the action, and an optional key context that scopes the binding:

```rust,ignore
use kael::{App, Application, KeyBinding};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Application::try_new()?.run(|cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("cmd-s", Save, None),
            KeyBinding::new("cmd-z", Undo, None),
            KeyBinding::new("cmd-shift-z", Redo, None),
            KeyBinding::new("tab", Tab, Some("Editor")),
            KeyBinding::new("shift-tab", TabPrev, Some("Editor")),
        ]);
        // ... open windows ...
    });
    Ok(())
}
```

Keystroke syntax uses `cmd` / `ctrl` / `alt` / `shift` modifiers joined with `-`, and a space separates multi-key sequences (e.g. `"cmd-k cmd-s"`). Use `cmd` on macOS and `ctrl` on Windows/Linux.

## Command Registry

Use `CommandRegistry` when the same app command should be available from a
command palette, menu, toolbar, or agent action list:

```rust
use kael::app_runtime::CommandRegistry;

let mut commands = CommandRegistry::new();
commands.register_action_checked("editor.save", "Save", || {
    // persist the active document
})?;

commands.execute("editor.save")?;
```

Prefer `register_checked(...)` and `register_action_checked(...)` for generated
app chrome. Checked registration rejects empty, padded, overly long, or
non-portable command IDs, rejects empty/padded/control-character/overly long
names, and catches duplicate IDs before menus or command palettes become
ambiguous. Raw `register(...)` and `register_action(...)` remain available when
an app intentionally wants replacement semantics.

## Handling actions

In `render`, mark the element that owns a focus context with `track_focus`, then register handlers with `on_action(cx.listener(...))`. Handlers take `&mut self`, a reference to the action, the window, and the context:

```rust
use kael::{div, prelude::*, Context, FocusHandle, Render, Window};

struct Editor { focus_handle: FocusHandle }

impl Editor {
    fn on_save(&mut self, _: &Save, _window: &mut Window, cx: &mut Context<Self>) {
        // ... persist ...
        cx.notify();
    }
}

impl Render for Editor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(Self::on_undo))
            .child("editor surface")
    }
}
```

## Focus & tab order

Create focus handles from the context and arrange tab order with `tab_index` / `tab_stop`. Move focus from the window:

```rust
fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
    let items = vec![
        cx.focus_handle().tab_index(1).tab_stop(true),
        cx.focus_handle().tab_index(2).tab_stop(true),
        cx.focus_handle().tab_index(3).tab_stop(true),
    ];
    let focus_handle = cx.focus_handle();
    window.focus(&focus_handle);
    Self { focus_handle, items }
}
```

Window focus methods: `window.focus(&handle)`, `window.focus_next()` (Tab), and `window.focus_prev()` (Shift-Tab). Query state with `handle.is_focused(window)` and style focused elements with `.focus(|s| s.border_color(...))`.

## Dispatch resolution

An element's `.key_context("Editor")` scopes matching bindings to that part of
the tree. When a keystroke matches, Kael starts at the focused element and walks
through its ancestors until an `on_action` handler accepts the action. A closer
handler can therefore override application-level behavior without coupling the
keymap to a concrete view type.

Use global bindings for commands that are valid throughout the application and
context bindings for editor modes, dialogs, lists, and other surfaces where the
same keystroke has a local meaning. See the Astryx showcase for a complete
focus-navigation composition.

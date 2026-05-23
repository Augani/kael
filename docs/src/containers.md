# Containers & Overlays

Components for organizing content, managing layers, and showing floating UI.

---

## Modal

Controlled dialog overlay with backdrop, escape-to-dismiss, and click-outside handling:

```rust
use kael::modal;

modal("confirm-dialog", self.is_open)
    .label("Confirm action")
    .backdrop(hsla(0.0, 0.0, 0.0, 0.5))
    .dismiss_on_escape(true)
    .dismiss_on_click_outside(true)
    .render_with({
        let entity = entity.clone();
        move |state, _window, _cx| {
            div()
                .w(px(400.0))
                .p_6()
                .bg(rgb(0xffffff))
                .rounded(px(12.0))
                .shadow_xl()
                .flex().flex_col().gap_4()
                .child(div().text_lg().child("Are you sure?"))
                .child(div().child("This action cannot be undone."))
                .child(
                    div().flex().justify_end().gap_2()
                        .child(button("cancel").label("Cancel")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.is_open = false;
                                        cx.notify();
                                    });
                                }
                            }))
                        .child(button("confirm").label("Confirm")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.do_action();
                                        this.is_open = false;
                                        cx.notify();
                                    });
                                }
                            }))
                )
                .into_any_element()
        }
    })
    .on_change({
        let entity = entity.clone();
        move |open, _window, cx| {
            entity.update(cx, |this, cx| {
                this.is_open = *open;
                cx.notify();
            });
        }
    })
```

**ModalRenderState fields:** `open`, `label`, `focused`

---

## Popover

Anchored floating panel with positioning:

```rust
use kael::popover;

popover("color-picker")
    .anchor(|_window, _cx| {
        button("show-colors").label("Colors").into_any_element()
    })
    .popup(|_window, _cx| {
        div()
            .w(px(200.0))
            .p_3()
            .bg(rgb(0xffffff))
            .shadow_lg()
            .rounded(px(8.0))
            .child("Color picker content")
            .into_any_element()
    })
    .dismiss_on_escape(true)
    .dismiss_on_click_outside(true)
```

---

## Tabs

Tabbed content switcher with keyboard navigation:

```rust
use kael::tabs;

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditorTab { Code, Preview, Settings }

tabs("editor-tabs", self.active_tab, [
    TabItem::new(EditorTab::Code, "Code", |_w, _cx| {
        div().child("Code editor here").into_any_element()
    }),
    TabItem::new(EditorTab::Preview, "Preview", |_w, _cx| {
        div().child("Live preview").into_any_element()
    }),
    TabItem::new(EditorTab::Settings, "Settings", |_w, _cx| {
        div().child("Editor settings").into_any_element()
    }),
])
.on_change({
    let entity = entity.clone();
    move |tab, _window, cx| {
        entity.update(cx, |this, cx| {
            this.active_tab = *tab;
            cx.notify();
        });
    }
})
```

**TabRenderState fields:** `value`, `label`, `index`, `tab_count`, `selected`, `focused`

---

## Disclosure

Collapsible section (accordion):

```rust
use kael::disclosure;

disclosure("advanced-settings", self.expanded)
    .trigger(|_w, _cx| {
        div().child("Advanced Settings ▾").into_any_element()
    })
    .panel(|_w, _cx| {
        div().p_3().child("Hidden content here").into_any_element()
    })
    .on_change({
        let entity = entity.clone();
        move |open, _window, cx| {
            entity.update(cx, |this, cx| {
                this.expanded = *open;
                cx.notify();
            });
        }
    })
```

---

## Splitter

Draggable pane divider for resizable layouts:

```rust
use kael::splitter;

splitter("main-split", self.split_ratio)
    .on_change({
        let entity = entity.clone();
        move |ratio, _window, cx| {
            entity.update(cx, |this, cx| {
                this.split_ratio = *ratio;
                cx.notify();
            });
        }
    })
```

Use the ratio value to size adjacent panes:

```rust
let left_width = self.split_ratio * total_width;
div().flex().flex_row()
    .child(div().w(px(left_width)).child("Left pane"))
    .child(splitter("split", self.split_ratio).on_change(/* ... */))
    .child(div().flex_1().child("Right pane"))
```

---

## Context Menu

Right-click menus via `.context_menu()` on any `Div`:

```rust
div()
    .id("file-item")
    .child("document.txt")
    .context_menu(|menu| {
        menu.item("Open", |_w, cx| { /* handle open */ })
            .item("Rename", |_w, cx| { /* handle rename */ })
            .separator()
            .item("Delete", |_w, cx| { /* handle delete */ })
    })
```

---

## Tooltip

Hover information via `.tooltip()` on any `Div`:

```rust
div()
    .id("save-icon")
    .child(icon("save"))
    .tooltip("Save file (Cmd+S)")

// Custom tooltip content
div()
    .id("status")
    .child("●")
    .tooltip_element(|| {
        div()
            .p_2()
            .bg(rgb(0x1E1E1E))
            .text_color(rgb(0xffffff))
            .rounded(px(4.0))
            .child("Connected to server")
    })
```

---

## Layer

Managed layer system for in-window modals and popovers:

```rust
use kael::layer;

layer("notification-layer")
    .placement(LayerPlacement::Centered)
    .child(/* floating content */)
```

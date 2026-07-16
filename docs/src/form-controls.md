# Form Controls

Every form control follows the same pattern:
1. Create with `widget_name(id, value, ...)`
2. Chain builder methods for configuration
3. Add `.on_change()` for state updates
4. Optionally add `.render_with()` for custom visuals

All controls support keyboard navigation and accessibility out of the box.

---

## Button

A focusable, clickable element with label support.

```rust
use kael::button;

button("save-btn")
    .label("Save File")
    .on_click({
        let entity = entity.clone();
        move |_event, _window, cx| {
            entity.update(cx, |this, cx| {
                this.save();
                cx.notify();
            });
        }
    })
```

**Builder methods:**
| Method | Description |
|--------|-------------|
| `.label(text)` | Display text |
| `.disabled()` | Disable interaction |
| `.on_click(handler)` | Click handler `(\|event, window, cx\| { ... })` |
| `.render_with(renderer)` | Custom rendering with `ButtonRenderState` |

**ButtonRenderState fields:** `label: Option<SharedString>`, `focused: bool`, `disabled: bool`

Use `state.to_text()`, `has_label()`, and `label_len_bytes()` in custom
renderers when logging or testing generated button chrome. The summary exposes
focus, disabled state, label presence, and label byte length without logging the
button text.

---

## TextInput

Full-featured text field with selection, clipboard, undo/redo, and password masking.

```rust
use kael::text_input;

text_input("project_name", self.name.clone())
    .placeholder("Enter project name")
    .on_change({
        let entity = entity.clone();
        move |value, _window, cx| {
            entity.update(cx, |this, cx| {
                this.name = value;
                cx.notify();
            });
        }
    })
```

**Builder methods:**
| Method | Description |
|--------|-------------|
| `.placeholder(text)` | Placeholder text when empty |
| `.multi_line()` | Enable multiline editing |
| `.max_lines(n)` | Limit visible height |
| `.password()` | Mask input characters |
| `.mask(impl InputMask)` | Custom input normalization |
| `.on_change(handler)` | Text change handler `(\|value: SharedString, window, cx\|)` |
| `.on_submit(handler)` | Enter key handler `(\|value: SharedString, window, cx\|)` |
| `.render_with(renderer)` | Custom rendering with `TextInputRenderState` |

**TextInputRenderState fields:** `value`, `display_text`, `placeholder`, `showing_placeholder`, `focused`, `hovered`, `multi_line`, `outer_bounds`, `field_bounds`, `text_bounds`, `line_height`, `lines`, `selection_bounds`, `cursor_bounds`

**Custom rendering helpers on state:** `state.paint_selection(color, window)`, `state.paint_text(window, cx)`, `state.paint_cursor(color, window)`

For custom renderers, use `state.to_text()`, `value_len_bytes()`,
`display_text_len_bytes()`, `placeholder_len_bytes()`, `has_placeholder()`,
`is_empty()`, `is_masked_display()`, `line_count()`,
`selection_rect_count()`, `has_selection()`, and `has_cursor()` for
content-safe diagnostics. These summaries describe focus, placeholder,
multiline, selection, caret, masking, and line shape without logging the field
value, placeholder text, displayed password mask contents, selected text, or
geometry coordinates.

---

## RichText

Native formatted text for previews, feeds, mentions, links, inline chips, and
read-only editor surfaces:

```rust
use kael::{rich_text, HighlightStyle};

let body = rich_text()
    .selectable()
    .text("Welcome ")
    .styled("builder", HighlightStyle::default())
    .link("docs", "https://example.com/docs", |_, _| {})
    .mention("@sam", "user-42", |_, _| {})
    .code("cargo run")
    .build();

tracing::info!(summary = body.to_text(), "rich text");
```

**Builder methods:**
| Method | Description |
|--------|-------------|
| `.text(text)` | Plain text segment |
| `.styled(text, HighlightStyle)` | Highlighted text segment |
| `.link(text, target, handler)` | Clickable link entity |
| `.mention(text, payload, handler)` | Clickable mention entity |
| `.hashtag(text, payload, handler)` | Clickable hashtag entity |
| `.code(text)` | Inline code-styled segment |
| `.inline_element(element)` | Inline child element |
| `.inline_element_with_baseline(element, px)` | Inline child with explicit baseline |
| `.selectable()` | Enable native text selection |
| `.track_layout(layout)` | Track selection and geometry through `RichTextLayout` |
| `.selection_color(color)` | Override selection highlight color |

Use `to_text()`, `segment_count()`, `text_segment_count()`,
`text_len_bytes()`, `inline_element_count()`, `inline_baseline_count()`,
`highlighted_segment_count()`, `code_segment_count()`, `entity_count()`,
`link_count()`, `mention_count()`, `hashtag_count()`, `click_handler_count()`,
`is_selectable()`, `has_selection_color()`, and `has_element_id()` for
content-safe agent summaries before render. These summaries do not log text,
URLs, mentions, hashtags, code contents, or entity payloads.

---

## Checkbox

Three-state checkbox (checked, unchecked, indeterminate) with undo/redo.

```rust
use kael::checkbox;

checkbox("notifications", self.enabled)
    .label("Enable notifications")
    .on_change({
        let entity = entity.clone();
        move |checked, _window, cx| {
            entity.update(cx, |this, cx| {
                this.enabled = *checked;
                cx.notify();
            });
        }
    })
```

**Builder methods:**
| Method | Description |
|--------|-------------|
| `.label(text)` | Label text |
| `.indeterminate(bool)` | Show indeterminate state |
| `.disabled()` | Disable interaction |
| `.on_change(handler)` | State change `(\|&bool, window, cx\|)` |
| `.render_with(renderer)` | Custom rendering with `CheckboxRenderState` |

**CheckboxRenderState fields:** `checked`, `indeterminate`, `label`, `focused`, `disabled`

Use `state.to_text()`, `has_label()`, and `label_len_bytes()` in custom
renderers. The summary reports checked, indeterminate, focus, disabled, and
label-shape state without logging the label text.

---

## Toggle

Boolean on/off switch with undo/redo.

```rust
use kael::toggle;

toggle("dark_mode", self.dark_mode)
    .label("Dark mode")
    .on_change({
        let entity = entity.clone();
        move |on, _window, cx| {
            entity.update(cx, |this, cx| {
                this.dark_mode = *on;
                cx.notify();
            });
        }
    })
```

**Builder methods:**
| Method | Description |
|--------|-------------|
| `.label(text)` | Label text |
| `.disabled()` | Disable interaction |
| `.on_change(handler)` | State change `(\|&bool, window, cx\|)` |
| `.render_with(renderer)` | Custom rendering with `ToggleRenderState` |

**ToggleRenderState fields:** `on`, `label`, `focused`, `disabled`

Use `state.to_text()`, `has_label()`, and `label_len_bytes()` in custom
renderers. The summary reports on/off, focus, disabled, and label-shape state
without logging the label text.

---

## RadioGroup

Mutually exclusive option selection with generic value types.

```rust
use kael::radio_group;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Theme { Light, Dark, System }

radio_group("theme", self.theme, [
    (Theme::Light, "Light"),
    (Theme::Dark, "Dark"),
    (Theme::System, "System"),
])
.on_change({
    let entity = entity.clone();
    move |value, _window, cx| {
        entity.update(cx, |this, cx| {
            this.theme = *value;
            cx.notify();
        });
    }
})
```

**Builder methods:**
| Method | Description |
|--------|-------------|
| `.on_change(handler)` | Selection change `(\|&T, window, cx\|)` |
| `.render_with(renderer)` | Custom rendering per option with `RadioItemRenderState<T>` |

**RadioItemRenderState fields:** `value`, `label`, `index`, `option_count`, `selected`, `focused`, `disabled`

Use `state.to_text()`, `label_len_bytes()`, `is_first()`, and `is_last()` in
custom renderers. The summary reports option position, count, selection, focus,
disabled, and label byte length without logging option values or label text.

---

## Slider

Continuous or discrete value control with drag support.

```rust
use kael::slider;

slider("volume", self.volume)
    .min(0.0)
    .max(100.0)
    .step(5.0)
    .on_change({
        let entity = entity.clone();
        move |value, _window, cx| {
            entity.update(cx, |this, cx| {
                this.volume = *value;
                cx.notify();
            });
        }
    })
```

**Builder methods:**
| Method | Description |
|--------|-------------|
| `.min(f64)` | Minimum value (default: 0.0) |
| `.max(f64)` | Maximum value (default: 100.0) |
| `.step(f64)` | Keyboard increment (default: 1.0) |
| `.discrete()` | Snap to step values |
| `.vertical()` | Vertical orientation |
| `.disabled()` | Disable interaction |
| `.on_change(handler)` | Value change `(\|&f64, window, cx\|)` |
| `.render_with(renderer)` | Custom rendering with `SliderRenderState` |

**SliderRenderState fields:** `value`, `min`, `max`, `percentage`, `dragging`, `focused`, `disabled`

Use `state.to_text()`, `position_class()`, `is_at_min()`, and `is_at_max()` in
custom renderers. The summary reports coarse position, edge state, dragging,
focus, and disabled state without logging exact values, bounds, or fractions.

---

## Progress

Determinate or indeterminate progress indicator with custom paint support.

```rust
use kael::progress;

progress("download", self.downloaded_bytes as f64)
    .max(self.total_bytes as f64)
```

**Builder methods:**
| Method | Description |
|--------|-------------|
| `.max(f64)` | Maximum value (default: 1.0) |
| `.indeterminate()` | Show busy progress without a numeric value |
| `.render_with(renderer)` | Custom painting with `ProgressRenderState` |

**ProgressRenderState fields:** `value`, `max`, `percentage`, `indeterminate`

Use `state.to_text()`, `is_determinate()`, and `completion_class()` in custom
renderers or generated task UIs. The summary reports determinate/indeterminate
state and a coarse completion class without logging exact values, maximums, or
fractions.

---

## Tabs

Controlled tab list with caller-owned panels and customizable tab triggers.

```rust
use kael::{TabItem, tabs};

tabs("settings", self.section, [
    TabItem::new(Section::General, "General", general_panel()),
    TabItem::new(Section::Billing, "Billing", billing_panel()),
])
.on_change({
    let entity = entity.clone();
    move |section, _window, cx| {
        entity.update(cx, |this, cx| {
            this.section = *section;
            cx.notify();
        });
    }
})
```

**Builder methods:**
| Method | Description |
|--------|-------------|
| `.on_change(handler)` | Selection change `(\|&T, window, cx\|)` |
| `.render_tabs_with(renderer)` | Custom tab trigger rendering with `TabRenderState<T>` |

**TabRenderState fields:** `value`, `label`, `index`, `tab_count`, `selected`, `focused`

Use `state.to_text()`, `label_len_bytes()`, `is_first()`, and `is_last()` in
custom tab renderers. The summary reports tab position, count, selection, focus,
and label byte length without logging tab values or label text.

---

## Disclosure

Controlled expandable section with caller-owned trigger visuals and panel
content.

```rust
use kael::disclosure;

disclosure("advanced", self.advanced_open)
    .label("Advanced")
    .panel(advanced_panel())
    .on_change({
        let entity = entity.clone();
        move |open, _window, cx| {
            entity.update(cx, |this, cx| {
                this.advanced_open = *open;
                cx.notify();
            });
        }
    })
```

**Builder methods:**
| Method | Description |
|--------|-------------|
| `.label(text)` | Trigger label text |
| `.panel(element)` | Content shown while open |
| `.on_change(handler)` | Open-state change `(\|&bool, window, cx\|)` |
| `.render_with(renderer)` | Custom trigger rendering with `DisclosureRenderState` |

**DisclosureRenderState fields:** `open`, `label`, `focused`

Use `state.to_text()`, `has_label()`, and `label_len_bytes()` in custom trigger
renderers. The summary reports open, focus, label presence, and label byte
length without logging trigger label text.

---

## Modal

Controlled dialog overlay with caller-owned content, backdrop, and dismissal
policy.

```rust
use kael::modal;

modal("confirm-delete", self.confirming_delete)
    .label("Confirm delete")
    .dismiss_on_escape(true)
    .dismiss_on_click_outside(true)
    .render_with(|state, _window, _cx| {
        tracing::info!(summary = state.to_text(), "modal");
        confirm_delete_panel().into_any_element()
    })
    .on_change({
        let entity = entity.clone();
        move |open, _window, cx| {
            entity.update(cx, |this, cx| {
                this.confirming_delete = *open;
                cx.notify();
            });
        }
    })
```

**Builder methods:**
| Method | Description |
|--------|-------------|
| `.label(text)` | Dialog accessibility label |
| `.backdrop(color)` | Backdrop color behind the dialog |
| `.dismiss_on_click_outside(bool)` | Request dismissal on outside click |
| `.dismiss_on_escape(bool)` | Request dismissal on Escape |
| `.on_change(handler)` | Open-state change `(\|&bool, window, cx\|)` |
| `.render_with(renderer)` | Custom dialog rendering with `ModalRenderState` |

**ModalRenderState fields:** `open`, `label`, `focused`, `dismiss_on_click_outside`, `dismiss_on_escape`

Use `state.to_text()`, `has_label()`, `label_len_bytes()`, and
`dismissal_mode()` in custom dialog renderers. The summary reports open, focus,
label shape, and dismissal policy without logging dialog label text.

---

## Popover

Controlled anchored overlay for menus, pickers, help panels, and compact
inspectors.

```rust
use kael::popover;

popover("help", self.help_open)
    .render_anchor_with(|state, _window, _cx| {
        tracing::info!(summary = state.to_text(), "popover anchor");
        help_button(state.open).into_any_element()
    })
    .render_popup_with(|state, _window, _cx| {
        tracing::info!(summary = state.to_text(), "popover popup");
        help_panel(state.width).into_any_element()
    })
    .on_open_change({
        let entity = entity.clone();
        move |open, _window, cx| {
            entity.update(cx, |this, cx| {
                this.help_open = *open;
                cx.notify();
            });
        }
    })
```

**Builder methods:**
| Method | Description |
|--------|-------------|
| `.on_open_change(handler)` | Open-state change `(\|&bool, window, cx\|)` |
| `.render_anchor_with(renderer)` | Custom anchor rendering with `PopoverAnchorRenderState` |
| `.render_popup_with(renderer)` | Custom popup rendering with `PopoverPopupRenderState` |
| `.dismiss_on_click_outside(bool)` | Request dismissal on outside click |
| `.dismiss_on_escape(bool)` | Request dismissal on Escape |
| `.offset(point)` | Offset popup relative to anchor |

**PopoverAnchorRenderState fields:** `open`, `dismiss_on_click_outside`, `dismiss_on_escape`

**PopoverPopupRenderState fields:** `open`, `width`, `anchor_bounds`, `focused`, `dismiss_on_click_outside`, `dismiss_on_escape`

Use anchor `to_text()` plus popup `to_text()`, `has_anchor_bounds()`,
`width_class()`, and `dismissal_mode()` in custom renderers. The summaries
report open/focus state, geometry availability, coarse width, and dismissal
policy without logging exact widths, coordinates, or bounds.

---

## MenuButton

Anchored popup menu button with keyboard navigation and customizable trigger and
item rows.

```rust
use kael::{MenuButtonItem, menu_button};

menu_button("file-actions", [
    MenuButtonItem::new(Action::Rename, "Rename"),
    MenuButtonItem::new(Action::Delete, "Delete"),
])
.label("Actions")
.render_trigger_with(|state, _window, _cx| {
    tracing::info!(summary = state.to_text(), "menu trigger");
    menu_trigger(state.open).into_any_element()
})
.render_items_with(|state, _window, _cx| {
    tracing::info!(summary = state.to_text(), "menu item");
    menu_item_row(state.label, state.highlighted, state.disabled).into_any_element()
})
.on_select({
    let entity = entity.clone();
    move |action, _window, cx| {
        entity.update(cx, |this, cx| {
            this.perform(*action);
            cx.notify();
        });
    }
})
```

**Builder methods:**
| Method | Description |
|--------|-------------|
| `.label(text)` | Trigger label text |
| `.on_select(handler)` | Item selection `(\|&T, window, cx\|)` |
| `.render_trigger_with(renderer)` | Custom trigger rendering with `MenuButtonTriggerRenderState` |
| `.render_items_with(renderer)` | Custom item row rendering with `MenuButtonItemRenderState<T>` |

**MenuButtonTriggerRenderState fields:** `open`, `label`, `focused`

**MenuButtonItemRenderState fields:** `value`, `label`, `index`, `highlighted`, `disabled`

Use trigger and item `to_text()` helpers plus `has_label()`,
`label_len_bytes()`, and item `label_len_bytes()` in custom renderers. The
summaries report open/focus state, item index, highlight/disabled state, and
label byte lengths without logging trigger labels, item labels, or item values.

---

## Toast

Transient in-window notification displayed by a `ToastStack`.

```rust
use kael::{Toast, ToastPosition, ToastStack};
use std::time::Duration;

let toast = Toast::new("Saved")
    .body("Project settings updated")
    .duration(Duration::from_secs(4))
    .position(ToastPosition::BottomRight);

tracing::info!(summary = toast.to_text(), "toast");
toast_stack.update(cx, |stack, cx| stack.push(toast, window, cx));
```

**Builder methods:**
| Method | Description |
|--------|-------------|
| `.body(text)` | Optional secondary text |
| `.duration(duration)` | Auto-dismiss duration |
| `.position(position)` | Screen position |

Use `toast.to_text()`, `has_body()`, `title_len_bytes()`,
`body_len_bytes()`, `duration_class()`, and `position_key()` before pushing
generated notifications. The summary reports text lengths, body presence,
coarse duration, and position without logging title/body text or exact seconds.
`ToastPosition::to_text()` returns stable keys for tests and traces.

---

## Splitter

Controlled splitter handle for resizable panes with keyboard, drag, and
undo/redo support.

```rust
use kael::{px, splitter};

splitter("sidebar-width", self.sidebar_width)
    .min(px(180.0))
    .max(px(420.0))
    .step(px(8.0))
    .on_change({
        let entity = entity.clone();
        move |width, _window, cx| {
            entity.update(cx, |this, cx| {
                this.sidebar_width = *width;
                cx.notify();
            });
        }
    })
```

**Builder methods:**
| Method | Description |
|--------|-------------|
| `.min(px)` | Minimum splitter position |
| `.max(px)` | Maximum splitter position |
| `.step(px)` | Keyboard and drag snap increment |
| `.discrete()` | Snap drag updates down to step values |
| `.horizontal()` | Horizontal rule that moves vertically |
| `.on_change(handler)` | Position change `(\|&Pixels, window, cx\|)` |
| `.render_with(renderer)` | Custom rendering with `SplitterRenderState` |

**SplitterRenderState fields:** `value`, `min`, `max`, `vertical`, `percentage`, `dragging`, `focused`

Use `state.to_text()`, `orientation()`, `position_class()`, `is_at_min()`, and
`is_at_max()` in custom renderers. The summary reports orientation, coarse
position, edge state, dragging, and focus without logging exact pixel values or
fractions.

---

## Label

Text label that can forward focus to another control.

```rust
use kael::label;

let label = label("project-name-label")
    .text("Project name")
    .for_focus_handle(name_focus.clone());

tracing::info!(summary = label.to_text(), "label");
```

**Builder methods:**
| Method | Description |
|--------|-------------|
| `.text(text)` | Visible and accessible label text |
| `.for_focus_handle(handle)` | Focus target control when clicked |

Use `label.to_text()`, `has_text()`, `text_len_bytes()`,
`has_target_focus()`, and `child_count()` before rendering generated form rows.
The summary reports text presence, text length, focus-target presence, and
custom child count without logging label text or child contents.

---

## ScrollBar

Focusable scroll bar bound to a scrollable container through a `ScrollHandle`.

```rust
use kael::{scroll_bar, px};

scroll_bar("results-scroll", self.scroll_handle.clone())
    .step(px(48.0))
    .render_with(|state, bounds, window, _cx| {
        tracing::info!(summary = state.to_text(), "scroll bar");
        paint_custom_scrollbar(state, bounds, window);
    })
```

**Builder methods:**
| Method | Description |
|--------|-------------|
| `.horizontal()` | Render a horizontal scroll bar |
| `.step(px)` | Keyboard scroll increment |
| `.render_with(renderer)` | Custom rendering with `ScrollBarRenderState` |

**ScrollBarRenderState fields:** `vertical`, `logical_offset`, `max_offset`, `viewport_size`, `content_size`, `percentage`, `thumb_ratio`, `dragging`, `focused`, `opacity`

Use `state.to_text()`, `orientation()`, `has_overflow()`,
`position_class()`, `thumb_size_class()`, `opacity_class()`, `is_at_start()`,
and `is_at_end()` in custom renderers. The summary reports orientation,
overflow, coarse scroll position, thumb class, drag/focus state, and visibility
class without logging exact offsets, sizes, ratios, coordinates, or opacity.

---

## Lists

Virtualized native lists for large collections, file explorers, queues, and
reorderable settings.

```rust
use kael::{ListAlignment, ListState, list, px};

let list_state = ListState::new(items.len(), ListAlignment::Top, px(240.0));
tracing::info!(summary = list_state.to_text(), "list state");

list_state.set_scroll_handler(|event, _window, _cx| {
    tracing::info!(summary = event.to_text(), "list scroll");
});

list(list_state.clone(), |index, _window, _cx| {
    render_row(index).into_any_element()
})
```

**List helpers:**
| Helper | Description |
|--------|-------------|
| `ListState::to_text()` | Content-safe item count, alignment, viewport, visible-count, scroll, overflow, and position summary |
| `ListScrollEvent::to_text()` | Content-safe visible range, visible count, item count, and scrolled state |
| `UniformList::to_text()` | Content-safe item count, measurement index, decoration count, scroll tracking, and sizing summary |
| `UniformListScrollHandle::to_text()` | Content-safe pending scroll-to-item intent, strategy, offset-in-items, scrollability, and flip summary |
| `RecyclingList::to_text()` | Content-safe delegate item count, sizing, alignment, and coarse overdraw summary |
| `ListAlignment::to_text()` | Stable `top` / `bottom` key |
| `ListSizingBehavior::to_text()` | Stable `infer` / `auto` key |
| `ListHorizontalSizingBehavior::to_text()` | Stable horizontal sizing key |
| `ScrollStrategy::to_text()` | Stable `top` / `center` / `bottom` key |

Use `item_count()`, `alignment()`, `has_viewport()`, `visible_range()`,
`visible_item_count()`, `has_scroll_offset()`, `has_overflow()`, and
`scroll_position_class()` when generated list UIs, diagnostics, or agents need
stable state. Summaries report counts, item indexes/ranges, and coarse scroll
classes without logging row contents, measured heights, viewport pixels, or
scroll offsets.

Use `UniformList::to_text()` before rendering large fixed-row collections and
`UniformListScrollHandle::to_text()` before generated scroll-to-item commands.
Use `RecyclingList::to_text()` for heterogeneous feeds or inspectors that rely
on delegate-provided estimated heights. These wrapper summaries report list
configuration and scroll intent without rendering rows or logging row contents,
measured heights, exact overdraw pixels, viewport geometry, or scroll offsets.

For sortable lists, call `sortable_reorder_plan(source, insertion, count)` and
log `plan.to_text()` before applying generated reorder mutations. Inspect
`has_move()`, `is_noop()`, `is_out_of_range()`, and `target()` to separate
valid moves from no-op or invalid drops. Use `sortable_auto_scroll_class(...)`
for drag-edge diagnostics without logging pointer coordinates.

---

## Semantic Primitives and Navigation

Native app structure for menus, links, trees, panes, dialogs, alerts, and route
stacks without embedding DOM history or browser accessibility nodes.

```rust
use kael::{link, menu_item, tree_item, Navigator, Route, Transition};

let docs = link("docs")
    .label("Docs")
    .url("https://example.com/docs");
tracing::info!(summary = docs.to_text(), "semantic link");

let item = tree_item("src").label("src").selected(true).expanded(true);
tracing::info!(summary = item.to_text(), "tree item");

let nav = Navigator::new(Route::new("home", cx.new(|_| HomeView)));
tracing::info!(summary = nav.to_text(), "navigator");
```

**Semantic helpers:**
| Helper | Description |
|--------|-------------|
| `MenuEntry::to_text()` | Label presence/byte length, disabled state, callback wiring, and child count |
| `Link::to_text()` | Label/URL presence and byte lengths, disabled state, activation mode, and child count |
| `TreeItem::to_text()` | Label byte length, selected state, expansion state, disabled state, activation mode, and child count |
| `Route::to_text()` | Route-id byte length and memento presence |
| `RouteChangeEvent::to_text()` | Previous/current route presence, route-id byte lengths, and stack depth |
| `Transition::to_text()` | Stable transition key |
| `Navigator::to_text()` | Stack depth, current-route presence, current route-id byte length, and active transition key |

Use these summaries when generated app chrome, sidebars, command menus, route
stacks, or tree views need native structure that would otherwise be modeled with
DOM nodes and browser history. The summaries do not log menu labels, link URLs,
tree labels, route IDs, mementos, child contents, or callback internals.

---

## Select

Dropdown with popup menu, optional search, and generic value types.

```rust
use kael::select;

select("accent", self.accent, [
    (AccentColor::Blue, "Atlantic"),
    (AccentColor::Green, "Forest"),
    (AccentColor::Orange, "Ember"),
])
.placeholder("Choose an accent")
.searchable()
.on_change({
    let entity = entity.clone();
    move |value, _window, cx| {
        entity.update(cx, |this, cx| {
            this.accent = *value;
            cx.notify();
        });
    }
})
```

**Builder methods:**
| Method | Description |
|--------|-------------|
| `.placeholder(text)` | Placeholder when nothing selected |
| `.searchable()` | Enable type-to-filter in popup |
| `.on_change(handler)` | Selection change `(\|&T, window, cx\|)` |
| `.render_with(renderer)` | Custom trigger rendering with `SelectRenderState` |
| `.render_options_with(renderer)` | Custom option row rendering with `SelectOptionRenderState<T>` |
| `.render_popup_with(renderer)` | Custom popup shell with `SelectPopupRenderState` |
| `.render_search_with(renderer)` | Custom search field with `SelectSearchRenderState` |

**SelectRenderState fields:** `open`, `display_text`, `selected_label`, `placeholder`, `showing_placeholder`, `focused`

For custom renderers, use `SelectRenderState::to_text()`,
`SelectOptionRenderState::to_text()`, `SelectPopupRenderState::to_text()`, and
`SelectSearchRenderState::to_text()` for content-safe diagnostics. The helpers
report open/focus state, placeholder and selected-label presence, option index,
selected/highlighted state, filtered counts, highlighted/selected index
presence, search activity, and string byte lengths without logging display
text, option labels, placeholder text, search queries, option values, popup
widths, or coordinates.

---

## DatePicker

Calendar-based date selection with month/year navigation.

```rust
use kael::date_picker;
use time::Date;

date_picker("delivery", self.delivery_date)
    .on_change({
        let entity = entity.clone();
        move |date, _window, cx| {
            entity.update(cx, |this, cx| {
                this.delivery_date = *date;
                cx.notify();
            });
        }
    })
```

**Builder methods:**
| Method | Description |
|--------|-------------|
| `.on_change(handler)` | Date selection `(\|&Date, window, cx\|)` |
| `.render_with(renderer)` | Custom trigger rendering with `DatePickerRenderState` |
| `.render_days_with(renderer)` | Custom day cell rendering with `DatePickerDayRenderState` |
| `.render_popup_with(renderer)` | Custom popup shell with `DatePickerPopupRenderState` |
| `.render_header_with(renderer)` | Custom month header with `DatePickerHeaderRenderState` |
| `.render_nav_buttons_with(renderer)` | Custom month navigation buttons |
| `.render_weekdays_with(renderer)` | Custom weekday labels |

**DatePickerDayRenderState fields:** `date`, `day`, `selected`, `highlighted`, `disabled`

For custom renderers, use `DatePickerRenderState::to_text()`,
`DatePickerDayRenderState::to_text()`, `DatePickerPopupRenderState::to_text()`,
`DatePickerHeaderRenderState::to_text()`,
`DatePickerNavButtonRenderState::to_text()`, and
`DatePickerWeekdayRenderState::to_text()` for content-safe diagnostics. The
helpers report open/focus state, label byte lengths, selectable/selected/
highlighted day state, selected-highlighted relation, navigation availability,
button direction/enabled state, and weekday index without logging exact dates,
month names, weekday labels, button labels, popup widths, or coordinates.

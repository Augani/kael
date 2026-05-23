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
| `.render_with(renderer)` | Custom rendering per option with `RadioRenderState` |

**RadioRenderState fields:** `value`, `label`, `index`, `selected`, `focused`

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
| `.render_with(renderer)` | Custom trigger rendering |
| `.render_days_with(renderer)` | Custom day cell rendering with `DateCellRenderState` |

**DateCellRenderState fields:** `day`, `selected`, `highlighted`, `disabled`, `today`

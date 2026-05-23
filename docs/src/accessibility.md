# Accessibility

Kael's built-in widgets are accessible by default — every form control reports its role, state, and value to screen readers and supports full keyboard navigation.

## Built-in accessibility

All form controls automatically provide:
- **Roles:** Button reports as button, checkbox as checkbox, etc.
- **States:** Focused, disabled, checked, selected, expanded
- **Values:** Slider reports its numeric value, progress reports percentage
- **Labels:** Set via `.label()` builder method
- **Keyboard navigation:** Tab between controls, Space/Enter to activate

You get this for free when using the built-in widgets.

## Adding accessibility to custom elements

For custom div-based interactive elements, add accessibility attributes:

```rust
div()
    .id("custom-toggle")
    .role(AccessibilityRole::Switch)
    .aria_checked(self.is_on)
    .aria_label("Enable dark mode")
    .on_click(|_, _, cx| { /* toggle */ })
```

## Keyboard navigation

### Focus management

```rust
// Create a focus handle
let focus = cx.focus_handle();

div()
    .id("panel")
    .track_focus(&focus)
    .on_key_down(|event, window, cx| {
        match event.keystroke.key.as_str() {
            "enter" => { /* activate */ },
            "escape" => { /* cancel */ },
            _ => {}
        }
    })
```

### Tab stops

Controls with IDs are automatically tab-focusable. Custom tab order:

```rust
div()
    .id("first-field")
    .tab_index(1)

div()
    .id("second-field")
    .tab_index(2)
```

## Label association

Use the `label` element to associate labels with controls:

```rust
label("Email address", "email-input")
// Clicking the label focuses the associated input

text_input("email-input", self.email.clone())
```

## Screen reader announcements

```rust
// Announce to screen readers
window.announce("File saved successfully");
```

## Accessibility roles

| Role | Used by |
|------|---------|
| `Button` | `button()` |
| `Checkbox` | `checkbox()` |
| `Radio` | `radio_group()` options |
| `Slider` | `slider()` |
| `TextInput` | `text_input()` |
| `Switch` | `toggle()` |
| `Dialog` | `modal()` |
| `Tab` | `tabs()` |
| `TabPanel` | `tabs()` panel content |
| `ProgressBar` | `progress()` |
| `Menu` | context menus |
| `MenuItem` | menu items |
| `Tree` | tree views |
| `TreeItem` | tree items |

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
let accessibility = AccessibilityAttributes::switch("Enable dark mode", self.is_on);
accessibility.validate()?;

div()
    .id("custom-toggle")
    .accessibility(accessibility)
    .on_click(|_, _, cx| { /* toggle */ })
```

Semantic recipes are available for common custom controls:

```rust
AccessibilityAttributes::button("Save")
AccessibilityAttributes::link("Open documentation")
AccessibilityAttributes::checkbox("Email notifications", enabled)
AccessibilityAttributes::switch("Enable sync", enabled)
AccessibilityAttributes::radio_button("Compact", selected)
AccessibilityAttributes::slider("Volume", value, 0.0, 100.0, Some(1.0))
AccessibilityAttributes::progress_bar("Upload progress", progress, 0.0, 100.0)
AccessibilityAttributes::text_input("Search", query.clone())
```

Use `.validate()?` in tests or builder code to catch unlabeled interactive
controls, missing actions, and invalid ranges before the UI renders.

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

### Focus traps

For custom modals, popovers, command palettes, and inspector panels, use the
headless `FocusTrapController` so `Tab`, `Shift-Tab`, and `Escape` behave like
users expect without tying the behavior to one visual component:

```rust
use kael_ui::prelude::{FocusTrapAction, FocusTrapController};

let trap = FocusTrapController::modal();
let root_focus = cx.focus_handle();

if trap.should_autofocus() {
    window.focus(&root_focus);
}

div()
    .id("settings-dialog")
    .track_focus(&root_focus)
    .tab_index(0)
    .on_key_down(move |event, window, cx| {
        match trap.action_for_keystroke(&event.keystroke) {
            Some(FocusTrapAction::FocusNext) => {
                window.focus_next_in_group();
                cx.stop_propagation();
                window.prevent_default();
            }
            Some(FocusTrapAction::FocusPrevious) => {
                window.focus_prev_in_group();
                cx.stop_propagation();
                window.prevent_default();
            }
            Some(FocusTrapAction::Dismiss) => {
                close_settings(window, cx);
            }
            Some(FocusTrapAction::FocusFirst) | None => {}
        }
    })
    .child(/* first focusable child */)
    .child(/* second focusable child */)
```

Use `FocusTrapController::persistent()` for surfaces where Escape should not
dismiss, or `.dismiss_on_escape(false)` / `.autofocus(false)` to tune a trap.

## Assistive-technology actions

Advertised actions tell screen readers and other assistive technologies what a
custom element can do:

```rust
let attrs = AccessibilityAttributes::switch("Enable sync", enabled)
    .action(AccessibilityAction::Toggle);
```

When a platform adapter or custom integration receives a native action request,
Kael normalizes it into its action vocabulary. Apps can register handlers on
the window:

```rust
window.on_accessibility_action(
    accessibility_id,
    AccessibilityAction::Toggle,
    |request| {
        assert_eq!(request.action, AccessibilityAction::Toggle);
        toggle_sync();
    },
);

let requests = window.drain_accessibility_actions();
```

Value-setting actions carry payload data:

```rust
window.on_accessibility_action(
    slider_id,
    AccessibilityAction::SetValue,
    |request| {
        if let Some(AccessibilityActionPayload::NumericValue(value)) = request.payload {
            set_volume(value);
        }
    },
);

window.on_accessibility_action(
    search_id,
    AccessibilityAction::SetValue,
    |request| {
        if let Some(AccessibilityActionPayload::Value(value)) = request.payload {
            set_search_query(value);
        }
    },
);
```

For custom test harnesses or non-window integrations, route normalized requests
through `AccessibilityActionRouter`:

```rust
let node = attrs.to_node(accessibility_id);
let mut router = AccessibilityActionRouter::new();

router.on_action(accessibility_id, AccessibilityAction::Toggle, |request| {
    assert_eq!(request.action, AccessibilityAction::Toggle);
    toggle_sync();
});

router.dispatch_accesskit(accessibility_id, &node, accesskit::Action::Click);
```

`AccessibilityActionRequest::from_accesskit_for_node(...)` uses the node's
advertised actions to recover Kael-specific meaning when a platform action is
coarser than Kael's vocabulary. For example, AccessKit `Click` can normalize to
`Toggle` for a switch or `ShowMenu` for a combobox when that is what the node
declared.

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

## Platform support

Kael builds one cross-platform accessibility tree per window each frame and
hands it to the native platform layer. There is nothing to opt into: any
window that renders accessible widgets (or custom elements with
`AccessibilityRole`/`aria_*` attributes) is exposed automatically.

| Platform | Backend | Status |
|----------|---------|--------|
| macOS | [`accesskit_macos`] `SubclassingAdapter` over the window's `NSView` | Adapter-backed; serves a full `NSAccessibility` tree to VoiceOver |
| Linux | [`accesskit_unix`] AT-SPI2 adapter (one per window, x11 and wayland) | Adapter-backed; exposes the tree on the AT-SPI2 D-Bus bus to Orca |
| Windows | Hand-rolled UI Automation provider (`IRawElementProviderSimple`) | Native UIA, served via `WM_GETOBJECT` |

On macOS and Linux the tree is built once with AccessKit and the official
adapters translate it to the platform protocol; Windows keeps its dedicated
UIA provider. All three are driven from the same per-frame tree, so widget
roles, labels, values, and focus stay consistent across platforms.

Notes:

- **macOS** requires no special entitlement; VoiceOver reads the served tree
  directly. The adapter dynamically subclasses the `NSView`, so it coexists
  with the rest of the AppKit window.
- **Linux** uses `accesskit_unix`'s default `async-io` executor, which owns its
  own background thread for the zbus/AT-SPI2 connection — kael's executors are
  not involved. AT-SPI2 needs no special permission.
- Assistive-technology action requests can be normalized with
  `AccessibilityActionRequest` and routed with `AccessibilityActionRouter`.
  macOS and Linux adapter drains now normalize pending AccessKit requests
  against the current tree so Kael-specific actions such as `Toggle`,
  `ShowMenu`, and `Dismiss` survive platform delivery. Windows feeds standard
  UIA focus, invoke, toggle, expand/collapse, and range-value pattern calls
  into the same window route.

[`accesskit_macos`]: https://crates.io/crates/accesskit_macos
[`accesskit_unix`]: https://crates.io/crates/accesskit_unix

## Testing with a screen reader

### macOS (VoiceOver)

Turn VoiceOver on with `Cmd-F5`, then focus your window and navigate with
`Ctrl-Option-Arrow`. Each control should be announced with its role and value
(for example, "Enable notifications, checkbox, checked").

To inspect the served tree without VoiceOver, use Xcode's **Accessibility
Inspector** (Xcode → Open Developer Tool → Accessibility Inspector) and point
its target picker at your running app, or query the Accessibility API directly
(`AXUIElementCreateApplication(pid)` walking `kAXChildrenAttribute`). A window
that previously exposed only a single root group will now report the full
control hierarchy.

### Linux (Orca)

Start Orca (`orca &`) with your app running. Because Kael registers an
`accesskit_unix` adapter per window, the controls appear on the AT-SPI2 bus and
Orca announces them as you `Tab` through. The `accerciser` tool can also be used
to browse the live AT-SPI2 tree.

### Windows (Narrator)

Start Narrator with `Ctrl-Win-Enter`. The UI Automation provider answers
`WM_GETOBJECT`, so controls are announced by role and name. The **Accessibility
Insights for Windows** tool can inspect the UIA tree.

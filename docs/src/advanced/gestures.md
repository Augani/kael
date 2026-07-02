# Gestures

Kael provides built-in gesture recognizers for touch and pointer interactions.
Before relying on tablet-style input, check the platform capability report:

```rust
let input = CapabilityCheck::new()
    .require(PlatformFeature::PrecisionPointerInput)
    .prefer_available(PlatformFeature::GestureInput)
    .prefer_available(PlatformFeature::TouchInput)
    .prefer_available(PlatformFeature::PenInput)
    .evaluate(&CapabilityReport::current());
```

Pointer, scroll, and magnify gestures are the portable baseline today. Direct
touch contact streams and pen pressure/tilt metadata are reported separately so
apps can provide mouse/keyboard fallbacks when a backend does not expose them.

## Pan gesture

Detect drag/pan movements with velocity tracking:

```rust
use kael::gesture::PanGesture;

let pan = PanGesture::new()
    .min_distance(px(5.0))
    .on_start(|position, _window, _cx| { /* drag started */ })
    .on_update(|delta, velocity, _window, _cx| { /* dragging */ })
    .on_end(|velocity, _window, _cx| { /* drag ended */ });
```

## Swipe gesture

Detect directional swipes:

```rust
use kael::gesture::SwipeGesture;

let swipe = SwipeGesture::new()
    .on_swipe(|direction, _window, _cx| {
        match direction {
            SwipeDirection::Left => { /* swipe left */ },
            SwipeDirection::Right => { /* swipe right */ },
            SwipeDirection::Up => { /* swipe up */ },
            SwipeDirection::Down => { /* swipe down */ },
        }
    });
```

## Pinch gesture

Zoom/scale with pinch-to-zoom or Ctrl+scroll:

```rust
use kael::gesture::PinchGesture;

let pinch = PinchGesture::new()
    .on_pinch(|scale, center, _window, _cx| {
        // scale: f64 (1.0 = no change, >1 = zoom in, <1 = zoom out)
        // center: Point<Pixels> (pinch center point)
    });
```

## Drag and drop

### File drop (from OS)

```rust
let filter = FileDropFilter::video().max_files(1);

div()
    .id("drop-zone")
    .can_drop_external(filter.clone())
    .on_external_drop(move |data, _window, _cx| {
        if let Some(paths) = data.accepted_paths_by(&filter) {
            for path in paths {
                /* import or open path */
            }
        }
        if let Some(text) = data.text_value() {
            /* handle dropped text */
        }
        for url in data.urls() {
            /* handle dropped URL */
        }
    })
```

Operating-system file drops are translated into Kael's typed drag/drop system as
`ExternalPaths` for file-only payloads and `ExternalDropData` when text or URLs
are present. Use `can_drop_external(filter)` and `on_external_drop(...)` to
handle both shapes through one browser-like payload. Presets are available for
common file cases: `FileDropFilter::single_file()`, `.images()`, `.audio()`,
`.video()`, and `.media()`.

For browser-style integrations that can carry text or URLs alongside files,
normalize to `ExternalDropData`:

```rust
let data = ExternalDropData::from_paths([path])
    .with_text("Dropped label")
    .with_url("https://example.com/item");

if let Some(paths) = data.accepted_paths_by(&FileDropFilter::images()) {
    /* import image paths */
}

let from_active_drag = ExternalDropData::from_drag_value(value);

let from_uri_list = ExternalDropData::from_uri_list(
    "file:///tmp/poster.png\nhttps://example.com/item\n",
);
let from_text = ExternalDropData::from_plain_text("https://example.com/item");
```

Native OS file-only drops still emit `ExternalPaths` for compatibility. Drops
that carry plain text or URLs emit `ExternalDropData` on macOS, Windows, and
Linux `text/uri-list` paths. Use `ExternalDropData` for custom platform
integrations, WebView bridge messages, and tests that need DataTransfer-like
`files` / `text` / `urls` payloads.

### Sortable reordering

See [SortableList](../lists-and-data.md#sortablelist) for drag-to-reorder within lists.

## Scroll events

```rust
div()
    .id("canvas")
    .on_scroll_wheel(|event, _window, _cx| {
        // event.delta: ScrollDelta (Pixels or Lines)
        // event.modifiers: Modifiers (detect Ctrl for zoom)
    })
```

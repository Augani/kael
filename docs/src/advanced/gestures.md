# Gestures

Kael provides built-in gesture recognizers for touch and pointer interactions.

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
div()
    .id("drop-zone")
    .on_file_drop(|event, _window, _cx| {
        match event {
            FileDropEvent::Entered(paths) => { /* files hovering */ },
            FileDropEvent::Submit(paths) => { /* files dropped */ },
            FileDropEvent::Exited => { /* drag cancelled */ },
            _ => {}
        }
    })
```

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

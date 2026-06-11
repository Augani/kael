# State Management

Kael's state model is built on entities: reference-counted, observable pieces of
state owned by the application. This chapter covers the full state toolkit —
entities, derived state with `Computed`, globals, and the patterns that keep a
large app consistent.

## Entities

An `Entity<T>` is a handle to a value owned by the app. Create one with
`cx.new`, read it with `read`, and mutate it with `update`:

```rust
struct Counter {
    count: usize,
}

let counter = cx.new(|_| Counter { count: 0 });

let value = counter.read(cx).count;

counter.update(cx, |counter, cx| {
    counter.count += 1;
    cx.notify();
});
```

`cx.notify()` is what tells Kael the entity changed: every view observing the
entity re-renders, and every computed value depending on it invalidates.
**Forgetting `cx.notify()` is the most common cause of a stale UI** — if you
mutated state and the screen didn't change, check for a missing notify first.

In event handlers inside a view, prefer `cx.listener` — it hands you `&mut Self`
and the right context without manual entity cloning:

```rust
.on_click(cx.listener(|this, _event, _window, cx| {
    this.count += 1;
    cx.notify();
}))
```

## Observing and subscribing

Views and entities react to each other with `observe` (any notify) and
`subscribe` (typed events from an `EventEmitter`):

```rust
cx.observe(&other_entity, |this, _other, cx| {
    this.recompute();
    cx.notify();
})
.detach();

cx.subscribe(&input, |this, _input, event: &InputEvent, cx| {
    if matches!(event, InputEvent::Change) {
        this.refilter(cx);
    }
})
.detach();
```

Both return a `Subscription` that unsubscribes when dropped — hold it in your
struct to scope it to the view's lifetime, or `.detach()` to keep it for the
emitter's lifetime.

## Derived state with `Computed`

`Computed<T>` is Kael's equivalent of a memo: a cached value derived from
entities, recomputed only when a dependency actually changes. Reads go through
the `Tracker` argument so dependencies are recorded automatically:

```rust
use kael::computed::Computed;

let filtered = Computed::new(cx, |tracker| {
    let orders = tracker.read(&orders_entity);
    let query = tracker.read(&search_entity).text.to_lowercase();
    orders
        .items
        .iter()
        .filter(|order| order.customer.to_lowercase().contains(&query))
        .cloned()
        .collect::<Vec<_>>()
});

let rows = filtered.read(cx);
```

The closure runs once; the result is cached until any tracked entity notifies.
Dependencies re-track on every recompute, so conditional reads work — a branch
that stops reading an entity stops depending on it.

A `Computed` is itself observable: `cx.observe(&filtered.entity(), ...)` lets a
view re-render when the derived value invalidates. Use `filtered.get(cx)` for a
cloned value.

Use `Computed` whenever you find yourself recomputing derived data inside
`render` — filtering, sorting, aggregating — so the work runs on change, not on
every frame.

## Globals

App-wide singletons implement the `Global` marker trait and live on the app:

```rust
struct Settings {
    telemetry: bool,
}
impl Global for Settings {}

cx.set_global(Settings { telemetry: false });
let settings = cx.global::<Settings>();
cx.update_global::<Settings, _>(|settings, _| settings.telemetry = true);
```

React to changes with `cx.observe_global::<Settings>(...)`. The kael_ui theme is
a global: read it with `Theme::of(cx)` (see [Theming](theming.md)).

## Structuring a larger app

- **One entity per unit of independent change.** A chat app wants
  `Entity<ChannelList>`, `Entity<Thread>`, `Entity<ComposerState>` — not one
  giant struct, which makes every keystroke re-render everything.
- **Derive, don't duplicate.** If a value can be computed from other state, use
  `Computed` rather than storing a second copy you must keep in sync.
- **Events for actions, observation for state.** Emit typed events for things
  that happen (message sent); observe entities for things that are (current
  draft).
- **Wire async through weak handles.** See [Async & Data Fetching](async-data.md)
  for the spawn/weak-update pattern and the `Loadable`/`QueryState` helpers.

A worked example lives in the dashboard template
(`templates/dashboard`): the search input subscribes to `InputEvent::Change`
and refilters the orders table through `DataTable::set_data`.

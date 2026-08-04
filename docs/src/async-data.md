# Async & Data Fetching

Kael apps stay responsive by doing work off the render path: futures run on the
foreground or background executor, and results land back in entities through
weak handles. kael_ui layers `Loadable` and `QueryState` on top so the common
fetch-render lifecycle needs no boilerplate.

## Tasks and executors

`cx.spawn` runs a future on the main thread with access to an async context;
`cx.background_spawn` runs CPU-bound or blocking work on the thread pool.

```rust
cx.spawn(async move |this, cx| {
    let data = cx
        .background_executor()
        .spawn(async move { expensive_parse(bytes) })
        .await;

    this.update(cx, |this, cx| {
        this.data = Some(data);
        cx.notify();
    })
    .ok();
})
.detach();
```

The closure receives a `WeakEntity<Self>` — the entity may be dropped while the
future runs, which is why every `update` through it returns a `Result`. Dropping
a `Task` cancels it; call `.detach()` to let it run to completion, or store it
in your struct so navigating away cancels in-flight work.

## Background Jobs

Use `JobScheduler` when work needs durable status, progress, retry metadata,
dependencies, cancellation, or a worker-pool handoff:

```rust
use kael::background_jobs::{JobDescriptor, JobPriority, JobScheduler, RetryPolicy};

let scheduler = JobScheduler::new().with_max_concurrent(2);
let descriptor = JobDescriptor::new("export/video")
    .with_priority(JobPriority::High)
    .with_retry_policy(RetryPolicy {
        max_retries: 2,
        delay_ms: 500,
        backoff_multiplier: 2.0,
    });

let job_id = scheduler.schedule_with_descriptor_checked(export_job, descriptor)?;
```

Prefer `schedule_checked(...)` or `schedule_with_descriptor_checked(...)` for
generated background work. Checked scheduling rejects empty, padded,
control-character, overly long, or non-portable job IDs, descriptor/job ID
mismatches, self-dependencies, duplicate or invalid dependency IDs, and invalid
retry policies before the queue state is mutated. Raw `schedule(...)` and
`schedule_with_descriptor(...)` remain available when an app owns validation.

## Loadable: the four states of remote data

`kael_ui::query::Loadable<T>` models the lifecycle every fetched value goes
through:

```rust
use kael_ui::prelude::Loadable;

match &self.orders {
    Loadable::Idle => div().child("Press fetch"),
    Loadable::Loading => Skeleton::new("orders-skeleton").into_any_element(),
    Loadable::Loaded(orders) => render_orders(orders),
    Loadable::Error(message) => Banner::error(message.clone()),
}
```

## QueryState: fetch lifecycle without the footguns

`QueryState<T>` owns a `Loadable<T>` and manages the transitions: it sets
`Loading`, spawns the fetch, writes the result back through a weak handle, and
**drops stale responses** when a newer fetch started (a generation counter —
no flash of old results when the user types fast). It supports `debounce` and
`refetch`.

```rust
use kael_ui::query::QueryState;

struct OrdersView {
    orders: QueryState<Vec<Order>>,
}

self.orders.run(cx, |cx| async move {
    fetch_orders(cx).await.map_err(|error| error.to_string().into())
});
```

For request dedupe across views, `QueryCache` keys results by string with a TTL.

The Astryx showcase composes query loading, data, refetch, and error states in
one application.

## Rules of thumb

- Never block the main thread: decode, parse, and diff on the background
  executor, then apply to entities on the foreground.
- Treat `WeakEntity::update` failures as cancellation, not errors — the view is
  gone; `.ok()` is the idiomatic acknowledgment.
- Hold the `Task` when navigation should cancel the request; detach when the
  result matters regardless.
- Set state to `Loading` *before* awaiting so the UI reflects the fetch
  immediately; `QueryState::run` does this for you.

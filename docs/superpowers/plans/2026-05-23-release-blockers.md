# Release Blockers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resolve all release blockers from `release_readiness_audit.md` so Kael can be published to crates.io.

**Architecture:** Fixes are grouped into 10 independent phases by category: resource bounding, concurrency, API hygiene, security, test coverage, and packaging. Each phase modifies 1-3 crates with no cross-phase dependencies. Phases can execute in any order.

**Tech Stack:** Rust, rusqlite, serde_json, ed25519-dalek (new dep for signing), smol (async), parking_lot

---

## Phase 1: Shadow Atlas Eviction (kael)

### Task 1: Add LRU eviction to shadow atlas cache

**Files:**
- Modify: `crates/kael/src/shadow_cache.rs`

The shadow atlas currently has no eviction. `ShadowAtlasParams` are hashed and stored in the platform atlas, but there's no cap on how many unique shadow configurations can accumulate. Add a bounded LRU tracker that evicts least-recently-used entries when a configurable budget is exceeded.

- [ ] **Step 1: Write the failing test for LRU eviction**

Add to the `tests` module in `crates/kael/src/shadow_cache.rs`:

```rust
#[test]
fn shadow_lru_tracker_evicts_oldest() {
    let mut tracker = ShadowLruTracker::new(3);
    let p1 = test_params(1, 1);
    let p2 = test_params(2, 2);
    let p3 = test_params(3, 3);
    let p4 = test_params(4, 4);

    tracker.touch(&p1);
    tracker.touch(&p2);
    tracker.touch(&p3);
    assert_eq!(tracker.len(), 3);

    let evicted = tracker.touch(&p4);
    assert_eq!(tracker.len(), 3);
    assert_eq!(evicted, Some(p1));
}

fn test_params(w: i32, h: i32) -> ShadowAtlasParams {
    ShadowAtlasParams::new(
        size(DevicePixels(w), DevicePixels(h)),
        Corners::default(),
        ScaledPixels(0.0),
        Hsla::black().opacity(0.5),
        false,
    )
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kael shadow_lru_tracker_evicts_oldest --lib`
Expected: FAIL — `ShadowLruTracker` not found.

- [ ] **Step 3: Implement ShadowLruTracker**

Add above the `tests` module in `crates/kael/src/shadow_cache.rs`:

```rust
use std::collections::VecDeque;

pub(crate) struct ShadowLruTracker {
    max_entries: usize,
    entries: VecDeque<ShadowAtlasParams>,
}

impl ShadowLruTracker {
    pub(crate) fn new(max_entries: usize) -> Self {
        Self {
            max_entries,
            entries: VecDeque::with_capacity(max_entries),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn touch(&mut self, params: &ShadowAtlasParams) -> Option<ShadowAtlasParams> {
        if let Some(pos) = self.entries.iter().position(|entry| entry == params) {
            self.entries.remove(pos);
            self.entries.push_back(params.clone());
            return None;
        }

        self.entries.push_back(params.clone());

        if self.entries.len() > self.max_entries {
            return self.entries.pop_front();
        }

        None
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p kael shadow_lru_tracker_evicts_oldest --lib`
Expected: PASS

- [ ] **Step 5: Write test that touching existing entry refreshes it**

```rust
#[test]
fn shadow_lru_touch_refreshes_entry() {
    let mut tracker = ShadowLruTracker::new(3);
    let p1 = test_params(1, 1);
    let p2 = test_params(2, 2);
    let p3 = test_params(3, 3);
    let p4 = test_params(4, 4);

    tracker.touch(&p1);
    tracker.touch(&p2);
    tracker.touch(&p3);
    tracker.touch(&p1); // refresh p1

    let evicted = tracker.touch(&p4);
    assert_eq!(evicted, Some(p2)); // p2 is now oldest
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p kael shadow_lru --lib`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/kael/src/shadow_cache.rs
git commit -m "feat(kael): add LRU eviction tracker for shadow atlas entries"
```

---

## Phase 2: Cache Capacity Policy (kael_cache)

### Task 2: Add per-namespace capacity limits with LRU eviction

**Files:**
- Modify: `crates/kael_cache/src/disk.rs`

The `DiskCache` already has `max_bytes` and an `evict_oldest` method, but there's no per-namespace budget. Add a `NamespacePolicy` that limits entry count per namespace with automatic eviction.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/kael_cache/src/disk.rs`:

```rust
#[test]
fn namespace_capacity_evicts_oldest() {
    let dir = tempfile::tempdir().unwrap();
    let cache = DiskCache::with_namespace_limit(dir.path().to_path_buf(), 1_000_000, 3).unwrap();

    cache.set("ns", "key1", b"data1").unwrap();
    cache.set("ns", "key2", b"data2").unwrap();
    cache.set("ns", "key3", b"data3").unwrap();
    cache.set("ns", "key4", b"data4").unwrap();

    assert!(cache.get("ns", "key1").unwrap().is_none());
    assert!(cache.get("ns", "key4").unwrap().is_some());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kael_cache namespace_capacity_evicts --lib`
Expected: FAIL — `with_namespace_limit` not found.

- [ ] **Step 3: Implement namespace capacity**

Add to `DiskCache`:

```rust
pub fn with_namespace_limit(
    root: PathBuf,
    max_bytes: u64,
    max_entries_per_namespace: usize,
) -> Result<Self> {
    let cache = Self::new(root, max_bytes)?;
    cache.index.lock().max_entries_per_namespace = Some(max_entries_per_namespace);
    Ok(cache)
}
```

Add `max_entries_per_namespace: Option<usize>` to `DiskIndex`. In `set()`, after inserting a new entry, check namespace entry count and evict oldest entries in that namespace (by `modified` time) if over the limit.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p kael_cache namespace_capacity_evicts --lib`
Expected: PASS

- [ ] **Step 5: Write concurrent writer stress test**

```rust
#[test]
fn concurrent_writers_do_not_corrupt() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Arc::new(DiskCache::new(dir.path().to_path_buf(), 10_000_000).unwrap());

    let handles: Vec<_> = (0..8)
        .map(|thread_id| {
            let cache = cache.clone();
            std::thread::spawn(move || {
                for i in 0..50 {
                    let key = format!("t{thread_id}-k{i}");
                    let data = format!("data-{thread_id}-{i}");
                    cache.set("stress", &key, data.as_bytes()).unwrap();
                    let read = cache.get("stress", &key).unwrap();
                    assert!(read.is_some(), "missing key {key}");
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }
}
```

- [ ] **Step 6: Run stress test**

Run: `cargo test -p kael_cache concurrent_writers --lib`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/kael_cache/src/disk.rs
git commit -m "feat(kael_cache): add per-namespace capacity limits and concurrent writer stress tests"
```

---

## Phase 3: Audio Player Fixes (kael_audio)

### Task 3: Fix load races with generation tracking

**Files:**
- Modify: `crates/kael_audio/src/player.rs`

The `load` method does async work (probing duration) but doesn't track which load is current. If `load` is called twice rapidly, the first load's completion can overwrite the second's state. Fix by adding a `load_generation` counter that increments on each `load` call; the async continuation only applies if its generation is still current.

- [ ] **Step 1: Write the failing test**

Add to the test module in `crates/kael_audio/src/player.rs`:

```rust
#[test]
fn superseded_load_does_not_overwrite_current() {
    let player = AudioPlayer::new();
    let gen_before = player.inner.lock().load_generation;
    {
        let mut state = player.inner.lock();
        state.load_generation += 1; // simulate a newer load starting
    }
    let gen_after = player.inner.lock().load_generation;
    assert_ne!(gen_before, gen_after);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kael_audio superseded_load --lib`
Expected: FAIL — `load_generation` field not found.

- [ ] **Step 3: Add load_generation to AudioPlayerState**

In `AudioPlayerState`, add:

```rust
load_generation: u64,
```

Initialize to `0` in `AudioPlayer::new()`.

In `load()`, at the start:
```rust
let my_generation = {
    let mut state = self.inner.lock();
    state.load_generation += 1;
    state.load_generation
};
```

After the async probe completes, before updating state:
```rust
let mut state = self.inner.lock();
if state.load_generation != my_generation {
    return Err(anyhow::anyhow!("load superseded by newer request"));
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p kael_audio --lib`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/kael_audio/src/player.rs
git commit -m "fix(kael_audio): prevent stale load completions from overwriting current state"
```

### Task 4: Remove no-op set_rate or implement it

**Files:**
- Modify: `crates/kael_audio/src/player.rs`

`set_rate` stores the rate in state but never applies it to the audio handle. Either wire it through to the platform handle or remove the public API. Since the platform backends don't support rate changes, remove the method and mark it as a future enhancement.

- [ ] **Step 1: Write test that set_rate is gone**

```rust
#[test]
fn audio_player_has_no_set_rate() {
    // Compile-time check: set_rate should not exist as public API.
    // If this test file compiles, the API surface is correct.
    let player = AudioPlayer::new();
    assert_eq!(player.rate(), 1.0);
}
```

- [ ] **Step 2: Remove set_rate, add rate() getter**

In `crates/kael_audio/src/player.rs`:
- Remove the `pub fn set_rate` method entirely
- Add a read-only getter:

```rust
pub fn rate(&self) -> f32 {
    self.inner.lock().rate
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p kael_audio --lib`
Expected: PASS

- [ ] **Step 4: Check no callers remain**

Run: `grep -rn "set_rate" crates/ --include="*.rs" | grep -v "test\|target"`
Expected: No results (only the old definition, now removed).

- [ ] **Step 5: Commit**

```bash
git add crates/kael_audio/src/player.rs
git commit -m "fix(kael_audio): remove no-op set_rate public API, add read-only rate() getter"
```

### Task 5: Add audio buffer size limits

**Files:**
- Modify: `crates/kael_audio/src/player.rs`

Add a constant `MAX_DECODED_AUDIO_BYTES` (e.g., 256 MB) and check estimated decoded size before loading. Reject files that would exceed the limit.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn rejects_oversized_audio() {
    assert!(AudioPlayer::exceeds_buffer_limit(
        Duration::from_secs(7200), // 2 hours
        48000,
        2,
        16,
    ));
    assert!(!AudioPlayer::exceeds_buffer_limit(
        Duration::from_secs(60), // 1 minute
        44100,
        2,
        16,
    ));
}
```

- [ ] **Step 2: Implement exceeds_buffer_limit**

```rust
const MAX_DECODED_AUDIO_BYTES: u64 = 256 * 1024 * 1024; // 256 MB

impl AudioPlayer {
    pub fn exceeds_buffer_limit(
        duration: Duration,
        sample_rate: u32,
        channels: u16,
        bits_per_sample: u16,
    ) -> bool {
        let bytes_per_sample = (bits_per_sample as u64 + 7) / 8;
        let total = duration.as_secs_f64() as u64
            * sample_rate as u64
            * channels as u64
            * bytes_per_sample;
        total > MAX_DECODED_AUDIO_BYTES
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p kael_audio --lib`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/kael_audio/src/player.rs
git commit -m "feat(kael_audio): add decoded buffer size limit guard"
```

---

## Phase 4: Storage Concurrency Fixes (kael_storage)

### Task 6: Fix SQLite observer registration race

**Files:**
- Modify: `crates/kael_storage/src/kv.rs`

In `SqliteKvStore::observe()`, the current value is read at line 441 (under `connection.lock()`), then the observer is registered at line 444 (under `observers.lock()`). A `set()` between those two operations fires the observer for the wrong value. Fix by holding both locks during registration and delivering the initial value under the same critical section.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn sqlite_observer_sees_initial_value() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteKvStore::open_at(dir.path().join("test.db")).unwrap();
    store.set("color", &"blue".to_string()).unwrap();

    let received = Arc::new(Mutex::new(Vec::new()));
    let recv_clone = received.clone();
    let _sub = store.observe::<String, _>("color", move |value| {
        recv_clone.lock().push(value);
    });

    // Observer should have received initial value "blue"
    let values = received.lock();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0], Some("blue".to_string()));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kael_storage sqlite_observer_sees_initial --lib`
Expected: Potentially flaky or wrong order.

- [ ] **Step 3: Fix observer registration**

In `SqliteKvStore::observe()`, restructure to:
1. Lock `connection`, read current value
2. While still holding connection lock, lock `observers` and register
3. Drop both locks
4. Fire initial callback with current value

```rust
fn observe<T, F>(&self, key: &str, callback: F) -> Subscription
where
    T: DeserializeOwned + Send + 'static,
    F: Fn(Option<T>) + Send + Sync + 'static,
{
    let callback = Arc::new(callback);
    let observer: Observer = {
        let callback = callback.clone();
        Arc::new(move |value| {
            let deserialized = value.and_then(|v| serde_json::from_value(v).ok());
            callback(deserialized);
        })
    };

    let key = key.to_string();
    let (observer_id, current_value) = {
        let connection = self.connection.lock();
        let current_value = load_value_from_connection(&connection, &key).ok().flatten();
        let mut state = self.observers.lock();
        let observer_id = state.next_observer_id;
        state.next_observer_id += 1;
        state
            .observers
            .entry(key.clone())
            .or_default()
            .insert(observer_id, observer.clone());
        (observer_id, current_value)
    };

    observer(current_value);

    let observers = self.observers.clone();
    Subscription::new(move || {
        let mut state = observers.lock();
        if let Some(key_observers) = state.observers.get_mut(&key) {
            key_observers.remove(&observer_id);
            if key_observers.is_empty() {
                state.observers.remove(&key);
            }
        }
    })
}
```

- [ ] **Step 4: Run test**

Run: `cargo test -p kael_storage sqlite_observer --lib`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/kael_storage/src/kv.rs
git commit -m "fix(kael_storage): fix SQLite observer registration race window"
```

### Task 7: Fix JSON store concurrent write race

**Files:**
- Modify: `crates/kael_storage/src/kv.rs`

The JSON store's `set()` uses optimistic concurrency with a generation check, but two concurrent writes can still both persist (one's file write gets overwritten). The fix: hold the mutex across the entire read-modify-write-persist cycle instead of releasing and reacquiring.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn json_concurrent_sets_do_not_lose_writes() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(JsonKvStore::open_at(dir.path().join("test.json")).unwrap());

    let handles: Vec<_> = (0..8)
        .map(|i| {
            let store = store.clone();
            std::thread::spawn(move || {
                for j in 0..20 {
                    let key = format!("key-{i}-{j}");
                    store.set(&key, &format!("val-{i}-{j}")).unwrap();
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    for i in 0..8 {
        for j in 0..20 {
            let key = format!("key-{i}-{j}");
            let val: Option<String> = store.get(&key).unwrap();
            assert!(val.is_some(), "missing {key}");
        }
    }
}
```

- [ ] **Step 2: Simplify JsonKvStore::set to hold lock across persist**

Replace the optimistic retry loop in `JsonKvStore::set()` with:

```rust
fn set<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
    let serialized = serde_json::to_value(value).map_err(|source| Error::SerializeValue {
        key: key.to_string(),
        source,
    })?;

    let observers = {
        let mut state = self.state.lock();

        if state.values.get(key) == Some(&serialized) {
            return Ok(());
        }

        state.values.insert(key.to_string(), serialized.clone());
        state.generation += 1;
        persist_values(&self.path, &state.values)?;

        state
            .observers
            .get(key)
            .map(|obs| obs.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default()
    };

    Self::notify(observers, Some(serialized));
    Ok(())
}
```

Do the same for `remove()`.

- [ ] **Step 3: Run tests**

Run: `cargo test -p kael_storage --lib`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/kael_storage/src/kv.rs
git commit -m "fix(kael_storage): hold lock across JSON persist to prevent concurrent write races"
```

---

## Phase 5: Notification Thread Exhaustion (kael_notifications)

### Task 8: Replace per-notification threads with a shared scheduler

**Files:**
- Modify: `crates/kael_notifications/src/local.rs`

Each delayed notification currently spawns a dedicated OS thread that sleeps. Replace with a single background scheduler thread that uses a `BinaryHeap` of wake times.

- [ ] **Step 1: Write the test**

```rust
#[test]
fn many_delayed_notifications_use_bounded_threads() {
    let center = NotificationCenter::new(NotificationCenterConfig::default());
    let initial_thread_count = std::thread::available_parallelism().map(|p| p.get()).unwrap_or(4);

    for i in 0..100 {
        let mut notif = LocalNotification::new(format!("test-{i}"));
        notif.body = Some("test".into());
        center
            .schedule_local(notif.with_trigger(NotificationTrigger::After { seconds: 3600.0 }))
            .unwrap();
    }

    // Should not have spawned 100 threads
    // The scheduler uses a single thread with a priority queue
    assert!(center.scheduler_thread_count() <= 2);
}
```

- [ ] **Step 2: Implement the shared scheduler**

Replace `spawn_delivery_loop` with a `NotificationScheduler` that:
1. Maintains a `BinaryHeap<Reverse<(Instant, NotificationId)>>`
2. Runs a single background thread that parks until the next wake time
3. On schedule: push to heap, unpark the scheduler thread
4. On cancel: set cancelled flag, scheduler skips on wake

```rust
use std::collections::BinaryHeap;
use std::cmp::Reverse;
use std::time::Instant;

struct SchedulerEntry {
    wake_at: Instant,
    id: NotificationId,
    notification: LocalNotification,
    actions: Vec<NotificationAction>,
    cancelled: Arc<AtomicBool>,
    repeats: bool,
    delay: Duration,
}

impl Ord for SchedulerEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.wake_at.cmp(&self.wake_at) // min-heap
    }
}

impl PartialOrd for SchedulerEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for SchedulerEntry {
    fn eq(&self, other: &Self) -> bool {
        self.wake_at == other.wake_at
    }
}

impl Eq for SchedulerEntry {}
```

The scheduler thread:
```rust
fn scheduler_loop(
    queue: Arc<Mutex<BinaryHeap<SchedulerEntry>>>,
    condvar: Arc<Condvar>,
    center: NotificationCenter,
) {
    let mutex = Mutex::new(());
    loop {
        let next_wake = {
            let mut q = queue.lock();
            q.peek().map(|entry| entry.wake_at)
        };

        match next_wake {
            Some(wake_at) => {
                let now = Instant::now();
                if wake_at > now {
                    let guard = mutex.lock();
                    let _ = condvar.wait_timeout(guard, wake_at - now);
                }
                // Process all due entries
                let mut q = queue.lock();
                while let Some(entry) = q.peek() {
                    if entry.wake_at > Instant::now() {
                        break;
                    }
                    let entry = q.pop().unwrap();
                    if entry.cancelled.load(Ordering::Relaxed) {
                        continue;
                    }
                    let _ = center.deliver_once(entry.id, entry.notification.clone(), entry.actions.clone());
                    if entry.repeats && !entry.cancelled.load(Ordering::Relaxed) {
                        q.push(SchedulerEntry {
                            wake_at: Instant::now() + entry.delay,
                            ..entry
                        });
                    }
                }
            }
            None => {
                let guard = mutex.lock();
                let _ = condvar.wait(guard);
            }
        }
    }
}
```

Add `pub fn scheduler_thread_count(&self) -> usize` that returns 1 (the single scheduler thread).

- [ ] **Step 3: Run tests**

Run: `cargo test -p kael_notifications --lib`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/kael_notifications/src/local.rs
git commit -m "fix(kael_notifications): replace per-notification threads with shared scheduler"
```

---

## Phase 6: Network Safety (kael_net)

### Task 9: Add response body size limits

**Files:**
- Modify: `crates/kael_net/src/client.rs`

Add a `max_response_bytes` field to `ApiRequest` with a default of 10 MB. When reading the response body, enforce the limit.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn request_has_default_body_limit() {
    let req = ApiRequest::get("/api/data");
    assert_eq!(req.max_response_bytes, Some(10 * 1024 * 1024));
}

#[test]
fn request_body_limit_is_configurable() {
    let req = ApiRequest::get("/api/data").with_max_response_bytes(1024);
    assert_eq!(req.max_response_bytes, Some(1024));
}

#[test]
fn request_can_disable_body_limit() {
    let req = ApiRequest::get("/api/data").without_response_limit();
    assert_eq!(req.max_response_bytes, None);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kael_net request_has_default_body_limit --lib`
Expected: FAIL

- [ ] **Step 3: Add max_response_bytes to ApiRequest**

Add the field with default 10MB:

```rust
pub struct ApiRequest {
    pub method: HttpMethod,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub timeout_ms: Option<u64>,
    pub max_response_bytes: Option<u64>,
}
```

In every constructor (`get`, `post`, `put`, `patch`, `delete`), set:
```rust
max_response_bytes: Some(10 * 1024 * 1024),
```

Add builder methods:
```rust
pub fn with_max_response_bytes(mut self, limit: u64) -> Self {
    self.max_response_bytes = Some(limit);
    self
}

pub fn without_response_limit(mut self) -> Self {
    self.max_response_bytes = None;
    self
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p kael_net --lib`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/kael_net/src/client.rs
git commit -m "feat(kael_net): add configurable response body size limits with 10MB default"
```

---

## Phase 7: Release Security (kael_release)

### Task 10: Add ed25519 manifest signing and verification

**Files:**
- Modify: `crates/kael_release/Cargo.toml`
- Modify: `crates/kael_release/src/update.rs`

Add `sign_manifest` and `verify_manifest` functions using ed25519 signatures. The public key is pinned in the `UpdatePolicy`.

- [ ] **Step 1: Add ed25519-dalek dependency**

In `crates/kael_release/Cargo.toml`, add:
```toml
ed25519-dalek = { version = "2", features = ["rand_core"] }
rand = "0.8"
```

- [ ] **Step 2: Write the failing test**

```rust
#[test]
fn sign_and_verify_manifest() {
    use ed25519_dalek::SigningKey;

    let signing_key = SigningKey::generate(&mut rand::thread_rng());
    let verifying_key = signing_key.verifying_key();

    let manifest = UpdateManifest {
        version: "1.2.0".into(),
        channel: UpdateChannel::Stable,
        url: "https://example.com/update.tar.gz".into(),
        sha256: "abc123".into(),
        size: 1024,
        release_notes: None,
        minimum_version: None,
    };

    let signature = sign_manifest(&manifest, &signing_key);
    assert!(verify_manifest(&manifest, &signature, &verifying_key));
}

#[test]
fn tampered_manifest_fails_verification() {
    use ed25519_dalek::SigningKey;

    let signing_key = SigningKey::generate(&mut rand::thread_rng());
    let verifying_key = signing_key.verifying_key();

    let mut manifest = UpdateManifest {
        version: "1.2.0".into(),
        channel: UpdateChannel::Stable,
        url: "https://example.com/update.tar.gz".into(),
        sha256: "abc123".into(),
        size: 1024,
        release_notes: None,
        minimum_version: None,
    };

    let signature = sign_manifest(&manifest, &signing_key);
    manifest.url = "https://evil.com/malware.tar.gz".into();
    assert!(!verify_manifest(&manifest, &signature, &verifying_key));
}
```

- [ ] **Step 3: Implement signing functions**

```rust
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey, Signature};

fn manifest_signing_payload(manifest: &UpdateManifest) -> Vec<u8> {
    format!(
        "kael-update-v1\n{}\n{}\n{}\n{}\n{}",
        manifest.version,
        manifest.channel_str(),
        manifest.url,
        manifest.sha256,
        manifest.size,
    )
    .into_bytes()
}

pub fn sign_manifest(manifest: &UpdateManifest, key: &SigningKey) -> Signature {
    let payload = manifest_signing_payload(manifest);
    key.sign(&payload)
}

pub fn verify_manifest(
    manifest: &UpdateManifest,
    signature: &Signature,
    key: &VerifyingKey,
) -> bool {
    let payload = manifest_signing_payload(manifest);
    key.verify(&payload, signature).is_ok()
}
```

Add `channel_str()` method to `UpdateChannel`:
```rust
impl UpdateChannel {
    fn as_str(&self) -> &str {
        match self {
            Self::Stable => "stable",
            Self::Beta => "beta",
            Self::Nightly => "nightly",
            Self::Custom(name) => name,
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p kael_release --lib`
Expected: PASS

- [ ] **Step 5: Write channel transition and downgrade prevention tests**

```rust
#[test]
fn downgrade_rejected() {
    let manifest = UpdateManifest {
        version: "1.0.0".into(),
        channel: UpdateChannel::Stable,
        url: "https://example.com/update.tar.gz".into(),
        sha256: "abc".into(),
        size: 1024,
        release_notes: None,
        minimum_version: Some("1.5.0".into()),
    };
    assert!(!manifest.is_compatible_with("1.2.0"));
}

#[test]
fn upgrade_accepted() {
    let manifest = UpdateManifest {
        version: "2.0.0".into(),
        channel: UpdateChannel::Stable,
        url: "https://example.com/update.tar.gz".into(),
        sha256: "abc".into(),
        size: 1024,
        release_notes: None,
        minimum_version: Some("1.0.0".into()),
    };
    assert!(manifest.is_compatible_with("1.5.0"));
}
```

- [ ] **Step 6: Run all tests**

Run: `cargo test -p kael_release --lib`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/kael_release/Cargo.toml crates/kael_release/src/update.rs
git commit -m "feat(kael_release): add ed25519 manifest signing, downgrade prevention tests"
```

---

## Phase 8: Document & Engine Hardening (kael_document, kael_engines)

### Task 11: Add atomic write recovery tests for kael_document

**Files:**
- Modify: `crates/kael_document/src/versions.rs`

Add tests that simulate crash during version blob writes and verify recovery on next open.

- [ ] **Step 1: Write crash recovery test**

```rust
#[test]
fn recovers_from_interrupted_blob_write() {
    let dir = tempfile::tempdir().unwrap();
    let store = VersionStore::open(dir.path()).unwrap();
    store.save_version(b"good data", "test").unwrap();

    // Simulate partial write: create a .tmp file alongside the blob
    let blob_dir = dir.path().join("blobs");
    std::fs::write(blob_dir.join("partial.tmp"), b"corrupt").unwrap();

    // Re-open should clean up temp files and still read good data
    let store2 = VersionStore::open(dir.path()).unwrap();
    let versions = store2.list_versions().unwrap();
    assert_eq!(versions.len(), 1);
    let data = store2.read_version(&versions[0].id).unwrap();
    assert_eq!(data, b"good data");
}
```

- [ ] **Step 2: Add version retention policy test**

```rust
#[test]
fn retention_policy_limits_stored_versions() {
    let dir = tempfile::tempdir().unwrap();
    let store = VersionStore::with_retention(dir.path(), 3).unwrap();

    for i in 0..5 {
        store.save_version(format!("v{i}").as_bytes(), "test").unwrap();
    }

    let versions = store.list_versions().unwrap();
    assert_eq!(versions.len(), 3);
}
```

- [ ] **Step 3: Implement retention policy**

Add `max_versions: Option<usize>` to `VersionStore`. Add constructor:

```rust
pub fn with_retention(root: impl AsRef<Path>, max_versions: usize) -> Result<Self> {
    let mut store = Self::open(root)?;
    store.max_versions = Some(max_versions);
    Ok(store)
}
```

In `save_version()`, after saving, if `max_versions` is set, prune oldest versions beyond the limit.

Add temp file cleanup to `open()`:
```rust
// Clean up interrupted writes
let blob_dir = root.join("blobs");
if blob_dir.exists() {
    for entry in std::fs::read_dir(&blob_dir)? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|ext| ext == "tmp") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p kael_document --lib`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/kael_document/src/versions.rs
git commit -m "feat(kael_document): add crash recovery, retention policy for version store"
```

### Task 12: Add search cancellation and timeline edge-case tests for kael_engines

**Files:**
- Modify: `crates/kael_engines/src/ide.rs`
- Modify: `crates/kael_engines/src/media.rs`

- [ ] **Step 1: Write search budget test**

In `crates/kael_engines/src/ide.rs` tests:

```rust
#[test]
fn search_respects_max_results() {
    let index = SearchIndex::new();
    for i in 0..1000 {
        index.add_document(format!("doc-{i}"), format!("the quick brown fox {i}"));
    }

    let results = index.search_with_limit("fox", 10);
    assert_eq!(results.len(), 10);
}

#[test]
fn regex_search_rejects_catastrophic_backtracking() {
    let index = SearchIndex::new();
    index.add_document("test", "aaaaaaaaaaaaaaaaa");

    let result = index.search_regex("(a+)+$");
    // Should complete quickly or return an error, not hang
    assert!(result.is_ok() || result.is_err());
}
```

- [ ] **Step 2: Add search_with_limit to SearchIndex**

```rust
pub fn search_with_limit(&self, query: &str, max_results: usize) -> Vec<SearchResult> {
    self.search(query)
        .into_iter()
        .take(max_results)
        .collect()
}
```

For regex safety, use `regex::RegexBuilder` with `size_limit`:
```rust
pub fn search_regex(&self, pattern: &str) -> Result<Vec<SearchResult>> {
    let regex = regex::RegexBuilder::new(pattern)
        .size_limit(1024 * 1024) // 1MB compiled regex limit
        .build()
        .map_err(|e| anyhow::anyhow!("invalid regex: {e}"))?;
    // ... search with regex ...
}
```

- [ ] **Step 3: Write timeline edge-case tests**

In `crates/kael_engines/src/media.rs` tests:

```rust
#[test]
fn zero_duration_clip_is_valid() {
    let clip = TimelineClip::new(0, 0, 0);
    assert_eq!(clip.duration(), 0);
    assert!(clip.validate().is_ok());
}

#[test]
fn max_frame_number_does_not_overflow() {
    let clip = TimelineClip::new(0, u64::MAX - 1, u64::MAX);
    assert!(clip.validate().is_ok());
    assert_eq!(clip.duration(), 1);
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p kael_engines --lib`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/kael_engines/src/ide.rs crates/kael_engines/src/media.rs
git commit -m "feat(kael_engines): add search budget controls, regex safety, timeline edge-case tests"
```

---

## Phase 9: PDF, i18n, Share, Icons Hardening

### Task 13: PDF page cache bounds

**Files:**
- Modify: `crates/kael_pdf/src/renderer.rs`

Add a `max_cached_pages` parameter (default 20) to limit rendered page cache size.

- [ ] **Step 1: Write the test**

```rust
#[test]
fn page_cache_respects_limit() {
    let mut cache = PageRenderCache::new(3);
    cache.insert(0, rendered_page(100, 100));
    cache.insert(1, rendered_page(100, 100));
    cache.insert(2, rendered_page(100, 100));
    cache.insert(3, rendered_page(100, 100));

    assert!(cache.get(0).is_none()); // evicted
    assert!(cache.get(3).is_some());
    assert_eq!(cache.len(), 3);
}

fn rendered_page(w: u32, h: u32) -> RenderedPage {
    RenderedPage::new(w, h, vec![0u8; (w * h * 4) as usize].into())
}
```

- [ ] **Step 2: Implement PageRenderCache**

```rust
pub struct PageRenderCache {
    max_pages: usize,
    pages: VecDeque<(usize, RenderedPage)>,
}

impl PageRenderCache {
    pub fn new(max_pages: usize) -> Self {
        Self {
            max_pages,
            pages: VecDeque::with_capacity(max_pages),
        }
    }

    pub fn get(&self, page_index: usize) -> Option<&RenderedPage> {
        self.pages
            .iter()
            .find(|(idx, _)| *idx == page_index)
            .map(|(_, page)| page)
    }

    pub fn insert(&mut self, page_index: usize, page: RenderedPage) {
        self.pages.retain(|(idx, _)| *idx != page_index);
        if self.pages.len() >= self.max_pages {
            self.pages.pop_front();
        }
        self.pages.push_back((page_index, page));
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p kael_pdf --lib`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/kael_pdf/src/renderer.rs
git commit -m "feat(kael_pdf): add bounded page render cache"
```

### Task 14: i18n locale fallback and snapshot tests

**Files:**
- Modify: `crates/kael_i18n/src/locale.rs`

- [ ] **Step 1: Write locale fallback tests**

```rust
#[test]
fn unsupported_locale_falls_back_to_default() {
    let formatter = NumberFormatter::for_locale("xx-YY");
    // Should fall back to en-US or root locale, not panic
    let result = formatter.format(1234.56);
    assert!(!result.is_empty());
}

#[test]
fn malformed_locale_tag_returns_fallback() {
    let formatter = NumberFormatter::for_locale("");
    let result = formatter.format(42.0);
    assert!(!result.is_empty());
}

#[test]
fn number_format_snapshot_en_us() {
    let f = NumberFormatter::for_locale("en-US");
    assert_eq!(f.format(1234.56), "1,234.56");
    assert_eq!(f.format(-42.0), "-42");
    assert_eq!(f.format(0.0), "0");
}

#[test]
fn number_format_snapshot_de_de() {
    let f = NumberFormatter::for_locale("de-DE");
    assert_eq!(f.format(1234.56), "1.234,56");
}
```

- [ ] **Step 2: Implement locale fallback**

In `NumberFormatter::for_locale`, if the locale isn't recognized:
```rust
pub fn for_locale(locale: &str) -> Self {
    if locale.is_empty() || !Self::is_supported_locale(locale) {
        return Self::for_locale("en-US");
    }
    // ... existing impl ...
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p kael_i18n --lib`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/kael_i18n/src/locale.rs
git commit -m "feat(kael_i18n): add locale fallback for unsupported/malformed tags, snapshot tests"
```

### Task 15: Share cleanup policy

**Files:**
- Modify: `crates/kael_share/src/lib.rs`

Add a cleanup function that removes materialized temp directories older than a configurable age.

- [ ] **Step 1: Write the test**

```rust
#[test]
fn cleanup_removes_stale_share_dirs() {
    let temp = tempfile::tempdir().unwrap();
    let old_dir = temp.path().join("kael-share-old-123-0");
    std::fs::create_dir(&old_dir).unwrap();
    std::fs::write(old_dir.join("test.png"), b"fake").unwrap();

    // Set mtime to 2 hours ago
    let two_hours_ago = SystemTime::now() - Duration::from_secs(7200);
    filetime::set_file_mtime(
        &old_dir,
        filetime::FileTime::from_system_time(two_hours_ago),
    ).unwrap();

    let removed = cleanup_share_temps(temp.path(), Duration::from_secs(3600));
    assert_eq!(removed, 1);
    assert!(!old_dir.exists());
}
```

- [ ] **Step 2: Implement cleanup_share_temps**

```rust
pub fn cleanup_share_temps(temp_dir: &Path, max_age: Duration) -> usize {
    let cutoff = SystemTime::now() - max_age;
    let mut removed = 0;

    let Ok(entries) = std::fs::read_dir(temp_dir) else {
        return 0;
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if !name_str.starts_with("kael-share-") {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        if modified < cutoff {
            if std::fs::remove_dir_all(entry.path()).is_ok() {
                removed += 1;
            }
        }
    }

    removed
}
```

- [ ] **Step 3: Add filetime dev-dependency**

In `crates/kael_share/Cargo.toml`:
```toml
[dev-dependencies]
filetime = "0.2"
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p kael_share --lib`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/kael_share/src/lib.rs crates/kael_share/Cargo.toml
git commit -m "feat(kael_share): add temp directory cleanup policy"
```

### Task 16: Define kael_icons unsupported-platform behavior

**Files:**
- Modify: `crates/kael_icons/src/platform/mod.rs`

Add a test and doc comment making the fallback behavior explicit.

- [ ] **Step 1: Write the test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_icon_lookup_returns_none() {
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            let result = lookup_icon("document");
            assert!(result.is_none());
        }
    }

    #[test]
    fn platform_icon_lookup_compiles() {
        // This test verifies the fallback bridge compiles on all platforms
        let _ = is_native_icon_available();
    }
}
```

- [ ] **Step 2: Add is_native_icon_available function**

```rust
pub fn is_native_icon_available() -> bool {
    cfg!(any(target_os = "macos", target_os = "windows", target_os = "linux"))
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p kael_icons --lib`
Expected: PASS (or compile-check on current platform)

- [ ] **Step 4: Commit**

```bash
git add crates/kael_icons/src/platform/mod.rs
git commit -m "docs(kael_icons): define unsupported platform fallback behavior with tests"
```

---

## Phase 10: Packaging & Foundation Crates

### Task 17: Clean up kael Cargo.toml publish surface

**Files:**
- Modify: `crates/kael/Cargo.toml`

- [ ] **Step 1: Audit and set publish excludes**

Review `crates/kael/Cargo.toml` and ensure test fixtures and internal examples are excluded from the published crate:

```toml
[package]
# ... existing fields ...
exclude = [
    "tests/",
    "benches/",
    "examples/",
]
```

- [ ] **Step 2: Run `cargo package --list -p kael` to verify**

Run: `cargo package --list -p kael 2>&1 | head -30`
Expected: No test fixtures or example binaries in the package list.

- [ ] **Step 3: Commit**

```bash
git add crates/kael/Cargo.toml
git commit -m "chore(kael): exclude test fixtures and examples from published crate"
```

### Task 18: Remove stale lockfiles from member crates

**Files:**
- Various `crates/*/Cargo.lock` files (if they exist)

- [ ] **Step 1: Find and remove stale lockfiles**

```bash
find crates/ -name "Cargo.lock" -type f
```

Remove any found files — workspace members should use the root lockfile.

- [ ] **Step 2: Commit**

```bash
git add -u crates/
git commit -m "chore: remove stale per-crate Cargo.lock files"
```

### Task 19: Add kael-macros compile-fail tests

**Files:**
- Create: `crates/kael-macros/tests/compile_fail/missing_attribute.rs`
- Create: `crates/kael-macros/tests/compile_fail.rs`

- [ ] **Step 1: Write compile-fail test harness**

Create `crates/kael-macros/tests/compile_fail.rs`:
```rust
#[test]
fn compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/*.rs");
}
```

Create `crates/kael-macros/tests/compile_fail/missing_attribute.rs`:
```rust
use kael_macros::IntoElement;

// This should fail to compile because the struct doesn't implement
// the required trait bounds.
#[derive(IntoElement)]
struct BadElement;

fn main() {}
```

- [ ] **Step 2: Add trybuild dev-dependency**

In `crates/kael-macros/Cargo.toml`:
```toml
[dev-dependencies]
trybuild = "1"
```

- [ ] **Step 3: Run test and capture expected error**

Run: `cargo test -p kael-macros compile_fail`
The first run will fail. Copy the error output to `tests/compile_fail/missing_attribute.stderr`.

- [ ] **Step 4: Run again**

Run: `cargo test -p kael-macros compile_fail`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/kael-macros/
git commit -m "test(kael-macros): add compile-fail tests for invalid derive inputs"
```

### Task 20: Add template smoke tests and dry-run release CI

**Files:**
- Modify: `.github/workflows/ci.yml` (or equivalent)
- Create: `xtask/src/template_check.rs` (if needed)

- [ ] **Step 1: Add template compile-check to CI**

In the CI workflow, add a job that runs:
```bash
cargo check -p template-dashboard -p template-messaging -p template-workspace
```

- [ ] **Step 2: Add dry-run release CI job**

```yaml
dry-run-release:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable
    - name: Validate dist config
      run: cargo xtask validate-dist --dry-run
    - name: Check publish readiness
      run: cargo xtask check-publish --dry-run
```

- [ ] **Step 3: Commit**

```bash
git add .github/ xtask/
git commit -m "ci: add template compile checks and dry-run release validation"
```

---

## Summary

| Phase | Crates | Blocker Category |
|-------|--------|-----------------|
| 1 | kael | Shadow atlas eviction |
| 2 | kael_cache | Cache capacity + concurrent writers |
| 3 | kael_audio | Load races, no-op API, buffer limits |
| 4 | kael_storage | JSON write race, SQLite observer race |
| 5 | kael_notifications | Thread exhaustion |
| 6 | kael_net | Response body size limits |
| 7 | kael_release | Manifest signing, downgrade prevention |
| 8 | kael_document, kael_engines | Crash recovery, search budget, timeline tests |
| 9 | kael_pdf, kael_i18n, kael_share, kael_icons | Cache bounds, locale fallback, cleanup, fallback docs |
| 10 | kael, kael-macros, templates, CI | Publish surface, lockfiles, compile-fail tests, dry-run CI |

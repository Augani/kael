# kael_storage

Durable application storage for Kael's native runtime.

The crate provides:

- `PlatformKvStore`, a SQLite-backed typed key-value store in the operating
  system's application-data location;
- `JsonKvStore` for applications that explicitly need a readable JSON file;
- SQLite connections, transactions, typed row mapping, and ordered migrations;
- deterministic platform paths on macOS, Windows, Linux, and FreeBSD.

It is independent of `kael_ui`: applications can use the storage battery with
Kael's runtime primitives and their own interface layer.

## Quick start

```rust,no_run
use kael_storage::{KvStore, PlatformKvStore};

fn main() -> kael_storage::Result<()> {
    let preferences = PlatformKvStore::open("com.example.product")?;

    preferences.set("theme", &"dark")?;
    let theme = preferences.get::<String>("theme")?;
    let keys = preferences.keys()?;

    assert_eq!(theme.as_deref(), Some("dark"));
    assert_eq!(keys, vec!["theme".to_string()]);
    Ok(())
}
```

Logical keys must be non-empty, no more than 4 KiB, and free of control
characters. Key enumeration and observer registration return `Result` so
backend failures are never disguised as empty data. Observer callbacks receive
`Result<Option<T>>`, which distinguishes a missing key from a value that does
not match the requested type. Notifications are serialized per observer and
the initial value is always delivered before concurrent updates.

Application and database identifiers are portable ASCII file names of at most
200 bytes. They may contain letters, numbers, spaces, `.`, `_`, and `-`, but
may not start or end with a space or `.`, and Windows device names such as
`CON` and `COM1` are rejected on every platform. Valid database names are
preserved exactly, avoiding collisions caused by lossy filename sanitization.

JSON updates are written to a temporary file in the destination directory,
flushed, synced, and atomically persisted over the previous file. The SQLite
backend enables write-ahead logging, a five-second busy timeout, and normal
synchronous durability.

See the [Kael guide](https://augani.github.io/kael/) for framework-level usage.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE-APACHE](LICENSE-APACHE).

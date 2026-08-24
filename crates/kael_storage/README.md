# kael_storage

Durable application storage for Kael's desktop and browser runtimes.

The crate provides:

- `PlatformKvStore`, a typed settings store backed by SQLite on desktop and one
  atomic `localStorage` entry in browsers;
- `BlobStore`, an asynchronous byte store backed by SQLite BLOB records on
  desktop and `Uint8Array` records in IndexedDB on the web;
- `JsonKvStore` for applications that explicitly need a readable JSON file;
- SQLite connections, transactions, typed row mapping, and ordered migrations;
- deterministic platform paths on macOS, Windows, Linux, and FreeBSD; and
- typed browser errors for native-path and SQLite operations that cannot keep
  their desktop semantics on the web.

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

## Binary and document data

`BlobStore` is the cross-platform choice for document bodies, images, project
archives, and other byte-oriented values:

```rust,no_run
use kael_storage::BlobStore;

# async fn example() -> kael_storage::Result<()> {
let documents = BlobStore::open("com.example.product", "documents").await?;
documents.put("draft", b"format-owned bytes").await?;
assert_eq!(documents.get("draft").await?.as_deref(), Some(&b"format-owned bytes"[..]));
# Ok(())
# }
```

Each record is bounded to 256 MiB. A successful mutation has committed its
SQLite or IndexedDB transaction before its future resolves. The browser store
uses raw `Uint8Array` records and does not base64-encode binary payloads.

## Browser contract

Browser `PlatformKvStore` is for settings and small metadata. Its serialized
map is bounded to 4 MiB so Kael reports a predictable size error before a
typical browser quota failure. `localStorage` quota, availability in private
browsing, and eviction remain browser policy. Store instances refresh from
`localStorage` on each operation; observers are local to the instance that
registered them.

`BlobStore` uses origin-scoped IndexedDB and surfaces denied access, quota
failures, transaction aborts, and corrupt record types as `Error::BrowserStorage`.
The maintained browser test opens a second connection and verifies committed
binary bytes survive it.

The native `Database`/`Transaction` SQL surface is deliberately not emulated.
On browser WebAssembly those entry points return `Error::BrowserSqlUnsupported`;
use `PlatformKvStore` or `BlobStore`, or integrate a domain-specific browser
database if the application genuinely requires SQL. Likewise, path-resolution
APIs return `Error::BrowserPathUnsupported` because an IndexedDB origin is not
a native directory.

See the [Kael guide](https://augani.github.io/kael/) for framework-level usage.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE-APACHE](LICENSE-APACHE).

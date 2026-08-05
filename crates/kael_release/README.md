# kael_release

Release metadata, signed update manifests, and fail-safe installation primitives
for native Kael applications.

This crate provides the release-side contracts used by Kael's optional updater:

- bounded application metadata and release-profile validation;
- Ed25519 signing and verification for update manifests;
- conservative update policies and explicit stable, beta, nightly, or custom channels;
- same-directory filesystem swaps with preflight checks and rollback;
- code-signature verification hooks, notarization status models, license reports,
  and software bills of materials.

`kael_release` does not publish crates or silently download software. The
`kael` crate's opt-in `auto-update` feature owns network retrieval and artifact
hash verification; applications remain responsible for when an update is
offered or installed.

```rust
use kael_release::update::{UpdateChannel, UpdateManifest, UpdatePolicy};

let policy = UpdatePolicy::default_stable();
policy.validate()?;

let manifest = UpdateManifest {
    version: "1.1.0".into(),
    channel: UpdateChannel::Stable,
    url: "https://downloads.example.com/app/1.1.0.tar.gz".into(),
    sha256: "a".repeat(64),
    size_bytes: 42_000_000,
    release_notes: Some("Performance and reliability improvements".into()),
    min_version: Some("1.0.0".into()),
};
manifest.validate()?;
# Ok::<(), anyhow::Error>(())
```

For the complete framework and guides, see the
[Kael repository](https://github.com/Augani/kael) and
[generated API documentation](https://docs.rs/kael_release).

## License

Licensed under the Apache License, Version 2.0. See [LICENSE-APACHE](LICENSE-APACHE).

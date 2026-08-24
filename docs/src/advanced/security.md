# Security & Permissions

Kael provides a capability-based security model for controlling what extensions and child processes can access.

## Permission system

```rust
use kael::security::*;

let mut manager = PermissionManager::new();

// Request permission
let request = PermissionRequest::new(
    PermissionKind::FileSystem,
    "Read project files",
);

match manager.check(&request) {
    PermissionStatus::Granted => { /* proceed */ },
    PermissionStatus::Denied => { /* blocked */ },
    PermissionStatus::Prompt => { /* ask user */ },
}
```

## Network policy

Control outbound network access:

```rust
let policy = NetworkPolicyBuilder::new()
    .allow_host("api.myapp.com")
    .allow_url("https://cdn.myapp.com/assets/app.js")?
    .build_checked()?;

assert!(policy.check_url("https://api.myapp.com/v1/sync")?);
assert!(!policy.check_url("https://evil.example.com/track")?);
```

Use `NetworkPolicy::DenyAll` for sandboxed workers by default, `AllowList`
for app-owned services, and `DenyList` only when most hosts should be allowed.
The checked builder rejects malformed hosts, full URLs in host fields,
non-HTTP(S) URLs, duplicate host entries, and mixed allow/deny lists.

## Process capabilities

Limit what child processes can do:

```rust
let limits = ProcessLimits {
    max_memory_bytes: Some(512 * 1024 * 1024),
    max_cpu_percent: Some(50.0),
    max_open_files: Some(256),
    network_allowed: true,
};

let mut capability = ProcessCapability::new(42, "worker", limits);
assert!(capability.check_network());
```

## File access bookmarks

Kael's standard UI threat model grants only `PathScope::UserSelected` file
read/write capabilities by default, so open/save dialogs and browser file
pickers work after an explicit user gesture. This does not grant arbitrary path
access: `PathScope::Any`, app-data access, and worker access still require an
explicit application policy or delegated bookmark.

Use file access bookmarks after open/save dialogs, recent-project restore, or
extension handoff flows. They keep app-owned path access explicit and can issue
temporary tokens instead of passing raw paths everywhere.

```rust
let bookmark = FileAccessBookmark::builder("workspace.main", workspace_dir)
    .scope(PathScope::UserSelected)
    .read_write()
    .require_existing_path()
    .canonicalize_path()
    .ttl_seconds(3600)
    .build_checked()?;

let mut tokens = AccessTokenStore::new();
let token = bookmark.issue_token(&mut tokens, now_unix_seconds)?;

for capability in bookmark.capabilities() {
    broker.grant(worker_process, capability);
}
```

## Credential storage

Secure credential management via OS keychain:

```rust
let keychain = KeychainStore::new("my-app");
keychain.write("api-token", "secret-value")?;
let token = keychain.read("api-token")?;
keychain.delete("api-token")?;
```

## Release dependency audit

Release CI runs `cargo audit -D warnings`, so a new vulnerability, unsoundness,
unmaintained dependency, or yanked crate fails the gate. The checked-in
`scripts/ci/audit-dependencies.sh` contains the complete reviewed exception
list. Cargo.lock still records Wry's target-conditional GTK3 metadata because
Wry is the supported Windows WebView2 host, including `RUSTSEC-2024-0429` for
glib 0.18. The audit script separately proves that neither Linux WebView feature
spelling can reach Wry, GTK3, WebKitGTK 4.1, or Blade. Linux ships only the
GTK4/WebKitGTK 6 host. The other reviewed exceptions are unmaintained
transitive parser/build crates, not known exploitable vulnerabilities. Remove
an exception as soon as the dependency that leaves the lockfile permits it.

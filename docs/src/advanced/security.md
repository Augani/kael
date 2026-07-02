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

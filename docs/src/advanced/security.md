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
let policy = NetworkPolicy {
    allowed_hosts: vec!["api.myapp.com".into()],
    blocked_hosts: vec![],
    allow_localhost: true,
};
```

## Process capabilities

Limit what child processes can do:

```rust
let limits = ProcessLimits {
    max_memory_mb: 512,
    max_cpu_percent: 50,
    max_open_files: 256,
};

let capabilities = vec![
    ProcessCapability::FileRead,
    ProcessCapability::Network,
];
```

## Credential storage

Secure credential management via OS keychain:

```rust
let keychain = KeychainStore::new("my-app");
keychain.write("api-token", "secret-value")?;
let token = keychain.read("api-token")?;
keychain.delete("api-token")?;
```

# kael_secrets

Secure credential storage backed by the macOS Keychain, Windows Credential
Manager, or freedesktop Secret Service on Linux.

```rust
use kael_secrets::SecretStore as _;

let store = kael_secrets::default_store()?;
store.set_string("com.example.app", "account", "token")?;
let token = store.get_string("com.example.app", "account")?;
if let Some(token) = token {
    send_authorization(token.expose_secret());
}
# fn send_authorization(_token: &str) {}
# Ok::<(), anyhow::Error>(())
```

Retrieved byte and string secrets use redacted, zeroizing wrappers. Input
buffers passed to `set_secret` or `set_string` remain the caller's
responsibility. Unsupported platforms return an error instead of silently
falling back to process-local storage.

`MemorySecretStore` is available for tests and unsupported platforms; it is
non-persistent and should not be used as a production keychain replacement.

Licensed under Apache-2.0.

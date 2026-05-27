# Plugins & Extensions

Kael ships an extension system with a contribution-point architecture. Extensions run out-of-process and can target one of two execution models: a sandboxed **WASM** module, or an **external process** that speaks the extension RPC protocol. The host loads extensions from a manifest, mediates their capabilities through a permission broker, and dispatches commands and notifications to them.

## Defining a manifest

Build a `PluginManifest` with the builder. The positional arguments are `id`, `name`, `version`, `api_version`, `entry_point`, and `execution_model`; contribution points and capabilities are added fluently.

```rust
use kael::{ContributedCommand, ExecutionModel, PluginManifest};

let manifest = PluginManifest::builder(
    "com.example.mock",   // id
    "Mock Plugin",        // display name
    "1.0.0",              // plugin version
    "1.0.0",              // host API version it targets
    "mock.wasm",          // entry point
    ExecutionModel::Wasm,
)
.command(ContributedCommand {
    id: "mock.hello".to_string(),
    title: "Say Hello".to_string(),
    keybinding: None,
})
.build()?;
```

Manifests can also be loaded from disk with `PluginManifest::from_json`, `PluginManifest::from_toml`, or `PluginManifest::load(path)`.

## Loading and activating

`ExtensionHostRuntime` owns the installed extensions for an app. Load a manifest, then activate it — `activate_with_broker` runs the extension's capability requests through a `PermissionBroker` first.

```rust
use kael::{ExtensionHostRuntime, PermissionBroker};

let mut runtime = ExtensionHostRuntime::new(&extensions_dir, "my-app");
runtime.load(manifest)?;

let broker = PermissionBroker::new();
for ext in runtime.all().iter().map(|e| e.manifest.id.clone()).collect::<Vec<_>>() {
    runtime.activate_with_broker(&ext, &broker)?;
}
```

Other runtime operations: `load_from_directory` (dev mode), `install_from_path`, `uninstall`, `activate` / `deactivate`, `unload`, `send_command`, `broadcast_notification`, and `all` (returns `&ExtensionInfo` with `manifest`, `is_active`, `process_id`, `load_path`, `dev_mode`).

## Contribution points

Extensions extend the host by contributing entries through the builder:

| Builder method | Contributes |
|----------------|-------------|
| `.command(ContributedCommand)` | A command (`id`, `title`, optional `keybinding`) |
| `.menu_item(ContributedMenuItem)` | A menu entry (`target_menu`, `label`, `command_id`) |
| `.panel(ContributedPanel)` | A panel/view (`id`, `title`, `default_position`) |
| `.settings_schema(json)` | A JSON schema for configurable settings |
| `.capability(Capability)` | A requested capability (e.g. `Capability::Notification`) |

Panels position with `PanelPosition::{Left, Right, Bottom, Floating}`.

## Execution models & RPC

`ExecutionModel::Wasm` runs the entry point as a sandboxed WebAssembly module. `ExecutionModel::ExternalProcess` launches a separate binary that connects over the platform transport and exchanges messages with the host.

The host and an external extension first complete a handshake (`ExtensionHandshake`, version-checked against `EXTENSION_RPC_VERSION`), then exchange a typed envelope:

| Direction | Type | Key variants |
|-----------|------|--------------|
| host → ext | `ExtensionRequest` | `Activate`, `Deactivate`, `Shutdown`, `GetContributions`, `ExecuteCommand { command_id, args }` |
| ext → host | `ExtensionResponse` | `Ack`, `Contributions(Contributions)` |
| host → ext | `ExtensionNotification` | `SettingsChanged { key, value }` |

Broadcast a notification to every active extension:

```rust
use kael::ExtensionNotification;

runtime.broadcast_notification(ExtensionNotification::SettingsChanged {
    key: "theme".to_string(),
    value: serde_json::json!("dark"),
});
```

See `examples/plugin_host.rs` for a complete host UI that loads both a WASM and an external-process extension, activates them through a broker, and streams a live log.

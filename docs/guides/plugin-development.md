# Plugin Development Guide

This guide covers how to create, package, and test plugins for the Kael framework.

## Overview

Kael plugins are self-contained extensions that add new capabilities to applications built on the framework. Plugins can contribute:

- Commands to the command palette
- Menu items
- Workspace panels
- Settings schemas

Plugins run in isolated processes (external native processes or WASM runtimes) and communicate with the host application via a typed IPC protocol.

## Plugin Manifest

Every plugin requires a manifest file (`manifest.json` or `manifest.toml`) that describes the plugin and its contributions.

### Minimal manifest (JSON)

```json
{
  "id": "com.example.my-plugin",
  "name": "My Plugin",
  "version": "1.0.0",
  "api_version": "1.0.0",
  "entry_point": "my-plugin",
  "execution_model": "ExternalProcess",
  "capabilities": [],
  "contributions": {}
}
```

### Fields

| Field | Required | Description |
|-------|----------|-------------|
| `id` | Yes | Reverse-DNS identifier (e.g., `com.example.plugin`) |
| `name` | Yes | Human-readable name |
| `version` | Yes | Semver version string |
| `api_version` | Yes | Target host API version (currently `1.0.0`) |
| `entry_point` | Yes | Executable path or WASM module path |
| `execution_model` | Yes | `ExternalProcess` or `Wasm` |
| `capabilities` | No | Array of requested capabilities |
| `args` | No | Command-line arguments for the entry point |
| `contributions` | No | UI contribution points |

### Execution models

- **ExternalProcess**: The plugin runs as a separate OS process. The host spawns it, passes an IPC socket path via the `Kael_EXTENSION_SOCKET` environment variable, and communicates over the RPC protocol.
- **Wasm**: The plugin runs inside a sandboxed WASM runtime. This is planned but not yet implemented.

## Contribution Points

### Commands

Commands appear in the command palette and can be bound to keyboard shortcuts.

```json
{
  "contributions": {
    "commands": [
      {
        "id": "my-plugin.say-hello",
        "title": "Say Hello",
        "keybinding": "cmd+shift+h"
      }
    ]
  }
}
```

### Menu items

Menu items attach to existing application menus.

```json
{
  "contributions": {
    "menu_items": [
      {
        "target_menu": "file",
        "label": "Do Thing",
        "command_id": "my-plugin.say-hello"
      }
    ]
  }
}
```

### Panels

Panels dock into the workspace layout.

```json
{
  "contributions": {
    "panels": [
      {
        "id": "my-plugin.sidebar",
        "title": "My Sidebar",
        "default_position": "Right"
      }
    ]
  }
}
```

Valid `default_position` values: `Left`, `Right`, `Bottom`, `Floating`.

## Capability Model

Plugins must declare the capabilities they need. The host application validates these against its `PermissionBroker` before activation.

### Available capabilities

- `OpenExternalUrl` — Open URLs in the default browser
- `FilesystemRead { scope }` — Read files (scope: `AppData`, `Downloads`, `UserSelected`, `Any`)
- `FilesystemWrite { scope }` — Write files (same scopes)
- `ShellExecute` — Execute shell commands (high-risk)
- `ClipboardRead` — Read clipboard (high-risk)
- `ClipboardWrite` — Write clipboard
- `Notification` — Show native notifications
- `Network { hosts }` — Make network requests (high-risk)
- `Microphone` — Access microphone
- `Camera` — Access camera
- `ScreenCapture` — Capture screen content (high-risk)

### Example capabilities declaration

```json
{
  "capabilities": [
    "Notification",
    { "FilesystemRead": { "scope": "AppData" } },
    { "Network": { "hosts": ["api.example.com"] } }
  ]
}
```

High-risk capabilities require explicit user or developer opt-in.

## RPC Protocol

External-process plugins communicate with the host via a versioned RPC protocol over Unix domain sockets (macOS/Linux) or named pipes (Windows).

### Environment variables

When spawned, the plugin receives:

- `Kael_EXTENSION_SOCKET` — IPC transport path
- `Kael_EXTENSION_ID` — The plugin identifier
- `Kael_API_VERSION` — Host API version

### Handshake

After connecting to the transport, the plugin must wait for a handshake message from the host and respond:

```rust
use kael::{
    ExtensionHandshake, ExtensionMessage, ExtensionTransport,
    EXTENSION_RPC_VERSION, UnixDomainSocketTransport,
};

let socket = std::env::var("Kael_EXTENSION_SOCKET").unwrap();
let transport = UnixDomainSocketTransport::connect(&socket).unwrap();
let mut transport = ExtensionTransport::new(Box::new(transport));

let msg = transport.recv_message().unwrap();
if let ExtensionMessage::Handshake(ExtensionHandshake::Host { .. }) = msg {
    transport.send_handshake(ExtensionHandshake::Extension {
        version: EXTENSION_RPC_VERSION,
        accepted: true,
    }).unwrap();
}
```

### Handling requests

The plugin receives requests and sends responses:

- `Activate` — The host wants the plugin to initialize
- `Deactivate` — The host wants the plugin to clean up
- `ExecuteCommand { command_id, args }` — Run a contributed command
- `GetContributions` — Return the current contribution set
- `Shutdown` — The process will be terminated

### Sending notifications

Plugins can send one-way notifications to the host:

- `CommandExecuted { command_id, result }`
- `PanelUpdated { panel_id, state }`
- `SettingsChanged { key, value }`

## Development Workflow

### Dev mode

During development, load a plugin directly from its source directory without installing:

```rust
let mut host = ExtensionHostRuntime::new("/app/extensions", "my-app");
host.load_from_directory("/path/to/plugin").unwrap();
```

Dev-mode plugins are not copied into the extensions directory.

### Installing for distribution

Package the plugin as a directory containing the manifest and any assets, then install:

```rust
host.install_from_path("/path/to/plugin").unwrap();
```

This copies the plugin into the host's extensions directory.

### Testing activation and permissions

Use `activate_with_broker` to validate capabilities before activation:

```rust
let broker = PermissionBroker::new();
broker.apply_threat_model(&ThreatModel::new());
host.activate_with_broker("com.example.plugin", &broker)?;
```

## Example Plugin Structure

```
my-plugin/
  manifest.json
  src/
    main.rs
  assets/
    icon.png
```

### manifest.json

```json
{
  "id": "com.example.my-plugin",
  "name": "My Plugin",
  "version": "1.0.0",
  "api_version": "1.0.0",
  "entry_point": "my-plugin",
  "execution_model": "ExternalProcess",
  "capabilities": ["Notification"],
  "contributions": {
    "commands": [
      {
        "id": "my-plugin.hello",
        "title": "Hello World",
        "keybinding": null
      }
    ],
    "panels": [
      {
        "id": "my-plugin.panel",
        "title": "My Panel",
        "default_position": "Right"
      }
    ]
  }
}
```

### main.rs

```rust
use kael::{
    ExtensionHandshake, ExtensionMessage, ExtensionRequest, ExtensionResponse,
    ExtensionTransport, EXTENSION_RPC_VERSION, UnixDomainSocketTransport,
};

fn main() {
    let socket = std::env::var("Kael_EXTENSION_SOCKET").unwrap();
    let transport = UnixDomainSocketTransport::connect(&socket).unwrap();
    let mut transport = ExtensionTransport::new(Box::new(transport));

    // Handshake
    let msg = transport.recv_message().unwrap();
    if let ExtensionMessage::Handshake(ExtensionHandshake::Host { .. }) = msg {
        transport.send_handshake(ExtensionHandshake::Extension {
            version: EXTENSION_RPC_VERSION,
            accepted: true,
        }).unwrap();
    }

    // Main loop
    loop {
        match transport.recv_message() {
            Ok(ExtensionMessage::Rpc(kael::process_model::IpcMessage::Request { id, body })) => {
                match body {
                    ExtensionRequest::Shutdown => {
                        transport.send_response(id, Ok(ExtensionResponse::Ack)).unwrap();
                        break;
                    }
                    ExtensionRequest::ExecuteCommand { command_id, .. } => {
                        println!("Executing: {}", command_id);
                        transport.send_response(id, Ok(ExtensionResponse::Ack)).unwrap();
                    }
                    _ => {
                        transport.send_response(id, Ok(ExtensionResponse::Ack)).unwrap();
                    }
                }
            }
            Err(_) => break,
            _ => {}
        }
    }
}
```

## Security Best Practices

1. Request only the capabilities your plugin actually needs.
2. Never assume the host will grant high-risk capabilities.
3. Handle `Shutdown` gracefully to avoid data loss.
4. Validate all inputs from the host before acting on them.
5. Keep your entry point executable minimal and focused.

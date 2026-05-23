# Plugins & Extensions

Kael supports a plugin system with WASM-sandboxed extensions and a contribution-point architecture.

## Extension manifest

Plugins declare their capabilities in a manifest:

```rust
use kael::plugin::*;

let manifest = ExtensionManifest {
    id: "my-plugin".into(),
    name: "My Plugin".into(),
    version: "1.0.0".into(),
    entry_point: "plugin.wasm".into(),
    contributions: vec![
        ContributionPoint::Command {
            id: "myPlugin.hello".into(),
            title: "Say Hello".into(),
        },
        ContributionPoint::Menu {
            items: vec![
                PluginMenuItem {
                    command_id: "myPlugin.hello".into(),
                    label: "Hello from Plugin".into(),
                    when: None,
                },
            ],
        },
    ],
    permissions: vec!["fs.read".into(), "network".into()],
};
```

## Extension registry

```rust
use kael::plugin::ExtensionRegistry;

let mut registry = ExtensionRegistry::new();
registry.register(manifest)?;

// Query extensions
let commands = registry.contribution_commands();
let themes = registry.contribution_themes();
```

## Contribution points

| Point | What it extends |
|-------|----------------|
| `Command` | Registers a new command |
| `Menu` | Adds items to menus |
| `Theme` | Contributes a color theme |
| `Language` | Adds language support |
| `Keybinding` | Registers keyboard shortcuts |
| `View` | Contributes a sidebar/panel view |
| `Setting` | Adds configuration options |

## Extension host

Extensions run in a sandboxed WASM environment with controlled access to the host application via `extension_rpc`.

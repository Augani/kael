# Showcase and starter applications

Kael keeps one maintained example instead of a large set of overlapping demo
binaries. The Astryx showcase is an interactive catalog of the framework's UI
surface, organized into focused sections for actions, inputs, selection, data
display, charts, feedback, navigation, overlays, typography, media, and layout.

```bash
git clone https://github.com/Augani/kael.git
cd kael
cargo run -p kael_ui --example astryx_showcase \
  --features "media kael/runtime_shaders"
```

The sidebar switches sections without launching another process. Controls are
live: you can type, drag, sort, resize, open overlays, navigate with the keyboard,
and inspect accessible states in the same application.

For deterministic visual or accessibility inspection, the showcase supports
section filters such as `ASTRYX_SHOWCASE_CHART_SECTION`,
`ASTRYX_SHOWCASE_OVERLAY_SECTION`, and `ASTRYX_SHOWCASE_LAYOUT_SECTION`.

## Starter applications

The workspace also contains three application templates. They are maintained as
real packages because they demonstrate application architecture rather than
isolated component examples:

```bash
cargo run -p dashboard-app
cargo run -p messaging-app
cargo run -p workspace-app
```

- `dashboard-app` combines application navigation, cards, charts, and tables.
- `messaging-app` combines a conversation list, message history, and composer.
- `workspace-app` combines a file tree, editor surface, panels, and status bar.

Create a standalone project from any template with the Kael CLI:

```bash
cargo run -p kael-cli -- new my-app
```

## Performance harness

The production performance workload is a Cargo benchmark, not an example:

```bash
cargo bench -p kael --bench framework
```

Use `scripts/bench/generate-baseline.sh` and
`scripts/bench/run-comparison.sh` to create and compare the checked workload.

Core platform workflows—windows, menus, capture, WebView, printing, plugins,
background processes, and release integration—live in the focused guide chapters
and automated tests. Keeping security- or platform-sensitive code there prevents
copy-pasted example implementations from drifting away from the supported API.

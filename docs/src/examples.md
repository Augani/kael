# Examples Gallery

Kael ships with 40+ runnable examples. Clone the repo and run any example:

```bash
git clone https://github.com/Augani/kael.git
cd kael
cargo run -p kael --example <name>
```

## Getting started

| Example | Command | What it shows |
|---------|---------|---------------|
| Hello World | `cargo run -p kael --example hello_world` | Minimal window with styled text and colored boxes |
| Form Controls | `cargo run -p kael --example form_controls` | Every form widget: text input, checkbox, toggle, slider, radio, select, date picker, modal |
| Input | `cargo run -p kael --example input` | Text input with custom rendering |

## Layout & styling

| Example | Command | What it shows |
|---------|---------|---------------|
| Grid Layout | `cargo run -p kael --example grid_layout` | CSS Grid-style layouts |
| Gradient | `cargo run -p kael --example gradient` | Linear and radial gradients |
| Shadow | `cargo run -p kael --example shadow` | Box shadow effects |
| Opacity | `cargo run -p kael --example opacity` | Transparency and blending |
| Pattern | `cargo run -p kael --example pattern` | Repeating pattern fills |
| Window | `cargo run -p kael --example window` | Window options and configuration |
| Window Positioning | `cargo run -p kael --example window_positioning` | Multi-display window placement |

## Lists & data

| Example | Command | What it shows |
|---------|---------|---------------|
| Data Table | `cargo run -p kael --example data_table` | Virtual data table with sorting and selection |
| Uniform List | `cargo run -p kael --example uniform_list` | High-performance uniform-height list |
| Recycling List | `cargo run -p kael --example recycling_list` | Variable-height virtualized list |
| Tree | `cargo run -p kael --example tree` | Expandable tree view |
| Scrollable | `cargo run -p kael --example scrollable` | Scroll containers with elastic scrolling |
| Elastic Scrolling | `cargo run -p kael --example elastic_scrolling` | Momentum and bounce scrolling |

## Text & rendering

| Example | Command | What it shows |
|---------|---------|---------------|
| Text | `cargo run -p kael --example text` | Text rendering and font features |
| Text Layout | `cargo run -p kael --example text_layout` | Text measurement and line breaking |
| Text Wrapper | `cargo run -p kael --example text_wrapper` | Word wrap and text overflow |
| Painting | `cargo run -p kael --example painting` | Custom GPU painting |
| SVG | `cargo run -p kael --example svg` | SVG rendering |
| Crispness Showcase | `cargo run -p kael --example crispness_showcase` | Pixel snapping, hairline strokes, sRGB gradients, corner shapes |
| Native Comparison | `cargo run -p kael --example native_comparison` | Side-by-side comparison with native AppKit rendering |

## Media & animation

| Example | Command | What it shows |
|---------|---------|---------------|
| Animation | `cargo run -p kael --example animation` | Keyframe and spring animations |
| GIF Viewer | `cargo run -p kael --example gif_viewer` | Animated GIF playback |
| Image | `cargo run -p kael --example image` | Image loading and display |
| Image Gallery | `cargo run -p kael --example image_gallery` | Gallery with lazy loading |
| Image Loading | `cargo run -p kael --example image_loading` | Async image loading patterns |

## Platform integration

| Example | Command | What it shows |
|---------|---------|---------------|
| Set Menus | `cargo run -p kael --example set_menus` | Native application menus |
| Tray Test | `cargo run -p kael --example tray_test` | System tray icon with menu |
| Platform Features | `cargo run -p kael --example platform_features` | Platform capability detection |
| Print Demo | `cargo run -p kael --example print_demo` | Native printing |
| WebView Demo | `cargo run -p kael --example webview_demo` | Embedded web content |
| Capture Demo | `cargo run -p kael --example capture_demo` | Screen/media capture |
| Drag & Drop | `cargo run -p kael --example drag_drop` | File drag-and-drop |

## Advanced

| Example | Command | What it shows |
|---------|---------|---------------|
| Plugin Host | `cargo run -p kael --example plugin_host` | Extension loading and management |
| Daemon App | `cargo run -p kael --example daemon_app` | Background daemon with tray |
| Tab Stop | `cargo run -p kael --example tab_stop` | Keyboard focus navigation |
| Window Shadow | `cargo run -p kael --example window_shadow` | Custom window chrome |
| On Window Close Quit | `cargo run -p kael --example on_window_close_quit` | Window lifecycle handling |

## Benchmarks

| Example | Command | What it shows |
|---------|---------|---------------|
| Perf Bench | `cargo run -p kael --example perf_bench --release` | Rendering performance measurement |
| Paths Bench | `cargo run -p kael --example paths_bench --release` | Path rendering performance |

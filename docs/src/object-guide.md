# Object guide

Use this page when you know what you want to build but not which Kael object to
reach for.

## Application objects

| Object | Purpose | Use it for |
|---|---|---|
| `Application` | Starts and owns the runtime | Process startup and the main event loop |
| `App` | Accesses application services | Windows, tasks, files, clipboard, and global state |
| `Window` | Handles one rendered surface | Focus, input, drawing, and window operations |
| `WindowOptions` | Describes a new window | Size, title, appearance, and placement |
| `CapabilityReport` | Reports platform support | Choosing a native path or a portable fallback |

## State and rendering

| Object | Purpose | Use it for |
|---|---|---|
| `Entity<T>` | Owns reactive application state | Models that outlive one render call |
| `Context<T>` | Reads and updates an entity | Listeners, notifications, tasks, and child entities |
| `Render` | Turns state into elements | Views with retained state |
| `RenderOnce` | Turns a value into elements once | Small value components and builders |
| `IntoElement` | Converts a value into a UI element | Return types from render methods |
| `Subscription` | Keeps an event listener alive | Observing entities, windows, and application events |
| `FocusHandle` | Identifies a focus target | Keyboard input and focus movement |

The normal update path is:

```text
input → entity.update(...) → cx.notify() → Render → retained scene → platform renderer
```

## Layout and components

| Object or function | Purpose | Use it for |
|---|---|---|
| `div()` | Creates the base layout element | Flex, grid, spacing, color, borders, and children |
| `Styled` | Adds style methods | Size, alignment, typography, and visual state |
| `kael_ui::init` | Registers the component system | Any app that uses `kael_ui` controls |
| `Theme` | Holds component design tokens | Product colors, type, radius, and density |
| `Button`, `Input`, `Select` | Ready made controls | Common interactive UI |
| `AppShell` | Creates an application frame | Sidebars, toolbars, and main content |

## Large data and graphics

| Object or function | Purpose | Use it for |
|---|---|---|
| `uniform_list` | Virtualizes equal height rows | Logs, feeds, and simple tables |
| `list` | Builds a measured list | Rows with variable height |
| `VirtualList` | Adds higher level virtual list behavior | Product lists using `kael_ui` |
| `VirtualSheetGrid` | Virtualizes rows and columns | Spreadsheet surfaces |
| `canvas` | Runs custom retained drawing | Charts, whiteboards, and editors |
| `Scene` | Stores platform render primitives | Low level retained output |
| `PortableScene2d` | Describes portable 2D scene data | Game and simulation foundations |

## Async, files, and web content

| Object | Purpose | Use it for |
|---|---|---|
| `Task<T>` | Represents scheduled async work | Fetching, parsing, saving, and delayed updates |
| `BackgroundExecutor` | Runs work away from UI rendering | CPU work that should not block a frame |
| `ExternalFile` | Carries a file name, type, and bytes | Portable open and drop workflows |
| `PrintJob` | Describes printable content | Native print dialogs and browser printing |
| `WebView` | Hosts a web owned surface | Compatibility islands and existing web products |

Start with `Application`, one root `Entity`, and a `Render` view. Add services
only when the feature needs them. See [Core concepts](core-concepts.md) for the
working code and [One codebase](one-codebase.md) for platform boundaries.

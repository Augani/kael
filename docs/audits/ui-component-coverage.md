# Kael UI component coverage

This ledger reconciles every top-level UI source module with the Astryx showcase and focused
interaction coverage. It covers 175 component modules, 10 data-display modules, 11 chart modules,
17 navigation modules, 12 overlay modules, and the root layout module.

Coverage means one of:

- a rendered Astryx example with representative states;
- a public helper/state/alias used by a rendered component;
- a compatibility module superseded by the supported prelude component.

| Area | Showcase coverage | Interaction / regression evidence | Status |
|---|---|---|---|
| Actions | Buttons, groups, menus, copy, FAB, pagination and role states | component interaction suite | complete |
| Inputs & forms | Text, number, date, range, time, picker, upload and editor families | input, picker and accessibility tests | complete |
| Selection | Checkbox, radio, switch, toggle, selector, combobox and typeahead families | keyboard/read-only/action tests | complete |
| Data display | Responsive production table, editable grid, rich text, structured search, metadata, timeline, list and tree surfaces | sorting, selection, edit-mode semantics, nested-scroll boundaries, compact/empty states and signed native inspection | complete |
| Charts | Bar (vertical/horizontal/grouped), line, area, pie/donut, gauge, radar, heatmap, contribution calendar, treemap and sparkline | 26 chart regression tests plus signed native inspection | complete |
| Feedback | Actionable alerts, banners, order-independent progress, counters, empty states, uncontrolled/controlled disclosure, skeleton/loading, indicators, toast and notification families | stable semantic-value, single-announcement, disclosure interaction and signed native inspection | complete |
| Navigation | Breadcrumbs, menus, hierarchical `NavigationMenu`, toolbar, tabs, trees, side/top/mobile navigation and virtual list | keyboard/action tests plus single-announcement semantics and signed native inspection of all seven isolated sections | complete |
| Overlays | Dialog, sheet, bottom sheet, popover, context menu, command palette, hover card, tooltip and toast viewport | focus, dismissal, identity, action and single-announcement semantics plus signed native inspection | complete |
| Typography | Text scale, headings, links, code, keyboard hints, quote, gradient and motion text | heading/link/range tests plus signed native inspection of all isolated sections | complete |
| Media | Icons, avatars, thumbnail, waveform, audio/video, viewer, decorative surfaces and interactive visual tools | media semantics, bounded-value tests and signed native inspection of all isolated/open states | complete |
| Layout | `VStack`, `HStack`, `Flow`, `Cluster`, `Spacer`, `Container`, `Panel`, Stack, Center, Section, Grid/GridSpan, masonry, panes, carousel, motion, sortable/infinite lists, `Draggable` and `DropZone` | geometry, keyboard drag/drop and signed native inspection | complete |

## Reconciled naming and compatibility exceptions

- `astryx_aliases.rs` contains supported aliases, not another render surface.
- `icon_source.rs` is the icon value model used by the rendered icon components.
- `otp_input.rs`, `qr_code.rs`, and `svg_renderer.rs` render as `OTPInput`, `QRCodeComponent`, and
  `SVGRenderer` in Astryx; filename-to-type matching alone reports false negatives.
- `text_input.rs` and `text_area.rs` are compatibility re-exports exercised through the Astryx
  `TextInput` and `TextArea` aliases.
- `display/rich_text.rs` is exercised through `render_blocks` and the rendered rich-block examples.
- `navigation/app_menu.rs` is exercised as the native `StandardMacMenuBar` attached to the signed
  showcase app.
- `components/nav_icon.rs` mirrors the rendered navigation `NavIcon` API.
- `components/confirm_dialog.rs` is a legacy module-level `Dialog`; the supported prelude API is
  `overlays::dialog::Dialog`, which has the complete focus, dismissal and accessibility coverage.

## Current verification

- `cargo test -p kael_ui --lib --tests --features kael/runtime_shaders`: 330 unit tests and 15
  integration interaction tests passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: all library, test, showcase, and
  standalone example targets passed with warnings denied.
- `time_picker_demo` and `astryx_showcase` use fallible application/window startup.
- Entitlement-signed native inspection covered every isolated Data Display and chart section,
  including responsive/no-result tables, editable grid semantics, line/horizontal charts,
  bounded gauges and the contribution calendar; every isolated Feedback section, including
  dismissible/action alerts, uncontrolled disclosure, notifications and progress semantics; every
  isolated Navigation section (tabs, disclosure, foundations, menus, toolbar, app chrome and compact
  navigation), including the opened mobile menu and deduplicated semantic labels; every isolated
  Overlay section and its dialog, side-sheet, bottom-sheet, command-palette and popover-menu open
  states; every isolated Typography section (with its marquee paused for deterministic native
  capture); every isolated Media section, including the open gallery and paused ambient/loading
  previews; and time picker, layout foundations and keyboard-capable drag/drop states.

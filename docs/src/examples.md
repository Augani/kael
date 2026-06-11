# Examples Gallery

Kael ships with **193 runnable examples** — 46 framework examples in `kael` and 147 component examples in `kael_ui`. Clone the repo and run any of them:

```bash
git clone https://github.com/Augani/kael.git
cd kael
```

> All commands use `runtime_shaders`, which compiles Metal shaders at launch so you don't need the full Xcode toolchain. See [Getting Started](getting-started.md) for details.

## Framework examples (`kael`)

Run with:

```bash
cargo run -p kael --example <name> --features runtime_shaders
```

| Example | Command |
|---------|---------|
| Animation | `cargo run -p kael --example animation --features runtime_shaders` |
| Capture Demo | `cargo run -p kael --example capture_demo --features runtime_shaders` |
| Content Effects | `cargo run -p kael --example content_effects --features runtime_shaders` |
| Crispness Showcase | `cargo run -p kael --example crispness_showcase --features runtime_shaders` |
| Daemon App | `cargo run -p kael --example daemon_app --features runtime_shaders` |
| Data Table | `cargo run -p kael --example data_table --features runtime_shaders` |
| Drag Drop | `cargo run -p kael --example drag_drop --features runtime_shaders` |
| Elastic Scrolling | `cargo run -p kael --example elastic_scrolling --features runtime_shaders` |
| Filters Gradients | `cargo run -p kael --example filters_gradients --features runtime_shaders` |
| Form Controls | `cargo run -p kael --example form_controls --features runtime_shaders` |
| Gif Viewer | `cargo run -p kael --example gif_viewer --features runtime_shaders` |
| Gradient | `cargo run -p kael --example gradient --features runtime_shaders` |
| Grid Layout | `cargo run -p kael --example grid_layout --features runtime_shaders` |
| Hello World | `cargo run -p kael --example hello_world --features runtime_shaders` |
| Image Gallery | `cargo run -p kael --example image_gallery --features runtime_shaders` |
| Image Loading | `cargo run -p kael --example image_loading --features runtime_shaders` |
| Input | `cargo run -p kael --example input --features runtime_shaders` |
| Native Comparison | `cargo run -p kael --example native_comparison --features runtime_shaders` |
| Objc2 Smoke | `cargo run -p kael --example objc2_smoke --features runtime_shaders` |
| On Window Close Quit | `cargo run -p kael --example on_window_close_quit --features runtime_shaders` |
| Opacity | `cargo run -p kael --example opacity --features runtime_shaders` |
| Ownership Post | `cargo run -p kael --example ownership_post --features runtime_shaders` |
| Painting | `cargo run -p kael --example painting --features runtime_shaders` |
| Paths Bench | `cargo run -p kael --example paths_bench --features runtime_shaders` |
| Pattern | `cargo run -p kael --example pattern --features runtime_shaders` |
| Perf Bench | `cargo run -p kael --example perf_bench --features runtime_shaders` |
| Platform Features | `cargo run -p kael --example platform_features --features runtime_shaders` |
| Plugin Host | `cargo run -p kael --example plugin_host --features runtime_shaders` |
| Print Demo | `cargo run -p kael --example print_demo --features runtime_shaders` |
| Recycling List | `cargo run -p kael --example recycling_list --features runtime_shaders` |
| Scrollable | `cargo run -p kael --example scrollable --features runtime_shaders` |
| Set Menus | `cargo run -p kael --example set_menus --features runtime_shaders` |
| Shadow | `cargo run -p kael --example shadow --features runtime_shaders` |
| Showcase 0 3 | `cargo run -p kael --example showcase_0_3 --features runtime_shaders` |
| Soft Ui | `cargo run -p kael --example soft_ui --features runtime_shaders` |
| Tab Stop | `cargo run -p kael --example tab_stop --features runtime_shaders` |
| Text | `cargo run -p kael --example text --features runtime_shaders` |
| Text Layout | `cargo run -p kael --example text_layout --features runtime_shaders` |
| Text Wrapper | `cargo run -p kael --example text_wrapper --features runtime_shaders` |
| Tray Test | `cargo run -p kael --example tray_test --features runtime_shaders` |
| Tree | `cargo run -p kael --example tree --features runtime_shaders` |
| Uniform List | `cargo run -p kael --example uniform_list --features runtime_shaders` |
| Webview Demo | `cargo run -p kael --example webview_demo --features runtime_shaders` |
| Window | `cargo run -p kael --example window --features runtime_shaders` |
| Window Positioning | `cargo run -p kael --example window_positioning --features runtime_shaders` |
| Window Shadow | `cargo run -p kael --example window_shadow --features runtime_shaders` |

## Component examples (`kael_ui`)

Run with:

```bash
cargo run -p kael_ui --example <name> --features kael/runtime_shaders
```

<details>
<summary><strong>Charts & data viz</strong> (5)</summary>

| Example | Command |
|---------|---------|
| Bar Chart Demo | `cargo run -p kael_ui --example bar_chart_demo --features kael/runtime_shaders` |
| Chart Demo | `cargo run -p kael_ui --example chart_demo --features kael/runtime_shaders` |
| Line Chart Demo | `cargo run -p kael_ui --example line_chart_demo --features kael/runtime_shaders` |
| Pie Chart Demo | `cargo run -p kael_ui --example pie_chart_demo --features kael/runtime_shaders` |
| Sparkline Demo | `cargo run -p kael_ui --example sparkline_demo --features kael/runtime_shaders` |

</details>

<details>
<summary><strong>Data display</strong> (25)</summary>

| Example | Command |
|---------|---------|
| Avatar Group Demo | `cargo run -p kael_ui --example avatar_group_demo --features kael/runtime_shaders` |
| Avatar Styled Demo | `cargo run -p kael_ui --example avatar_styled_demo --features kael/runtime_shaders` |
| Badge Styled Demo | `cargo run -p kael_ui --example badge_styled_demo --features kael/runtime_shaders` |
| Breadcrumbs Styled Demo | `cargo run -p kael_ui --example breadcrumbs_styled_demo --features kael/runtime_shaders` |
| Calendar Styled Demo | `cargo run -p kael_ui --example calendar_styled_demo --features kael/runtime_shaders` |
| Card Demo | `cargo run -p kael_ui --example card_demo --features kael/runtime_shaders` |
| Card Styled Demo | `cargo run -p kael_ui --example card_styled_demo --features kael/runtime_shaders` |
| Data Table Demo | `cargo run -p kael_ui --example data_table_demo --features kael/runtime_shaders` |
| Data Table Styled Demo | `cargo run -p kael_ui --example data_table_styled_demo --features kael/runtime_shaders` |
| Empty State Demo | `cargo run -p kael_ui --example empty_state_demo --features kael/runtime_shaders` |
| File Tree Demo | `cargo run -p kael_ui --example file_tree_demo --features kael/runtime_shaders` |
| Hover Card Styled Demo | `cargo run -p kael_ui --example hover_card_styled_demo --features kael/runtime_shaders` |
| Infinite Scroll Demo | `cargo run -p kael_ui --example infinite_scroll_demo --features kael/runtime_shaders` |
| Masonry Grid Demo | `cargo run -p kael_ui --example masonry_grid_demo --features kael/runtime_shaders` |
| Pagination Styled Demo | `cargo run -p kael_ui --example pagination_styled_demo --features kael/runtime_shaders` |
| Rating Demo | `cargo run -p kael_ui --example rating_demo --features kael/runtime_shaders` |
| Status Bar Demo | `cargo run -p kael_ui --example status_bar_demo --features kael/runtime_shaders` |
| Status Bar Styled Demo | `cargo run -p kael_ui --example status_bar_styled_demo --features kael/runtime_shaders` |
| Stepper Demo | `cargo run -p kael_ui --example stepper_demo --features kael/runtime_shaders` |
| Table Styled Demo | `cargo run -p kael_ui --example table_styled_demo --features kael/runtime_shaders` |
| Timeline Demo | `cargo run -p kael_ui --example timeline_demo --features kael/runtime_shaders` |
| Tree List Demo | `cargo run -p kael_ui --example tree_list_demo --features kael/runtime_shaders` |
| Tree Performance Demo | `cargo run -p kael_ui --example tree_performance_demo --features kael/runtime_shaders` |
| Tree Styled Demo | `cargo run -p kael_ui --example tree_styled_demo --features kael/runtime_shaders` |
| Virtual List Demo | `cargo run -p kael_ui --example virtual_list_demo --features kael/runtime_shaders` |

</details>

<details>
<summary><strong>Inputs & forms</strong> (29)</summary>

| Example | Command |
|---------|---------|
| Checkbox Styled Demo | `cargo run -p kael_ui --example checkbox_styled_demo --features kael/runtime_shaders` |
| Color Picker Demo | `cargo run -p kael_ui --example color_picker_demo --features kael/runtime_shaders` |
| Combobox Demo | `cargo run -p kael_ui --example combobox_demo --features kael/runtime_shaders` |
| Date Picker Demo | `cargo run -p kael_ui --example date_picker_demo --features kael/runtime_shaders` |
| Editor Demo | `cargo run -p kael_ui --example editor_demo --features kael/runtime_shaders` |
| Editor Styled Demo | `cargo run -p kael_ui --example editor_styled_demo --features kael/runtime_shaders` |
| File Upload Demo | `cargo run -p kael_ui --example file_upload_demo --features kael/runtime_shaders` |
| Hotkey Input Demo | `cargo run -p kael_ui --example hotkey_input_demo --features kael/runtime_shaders` |
| Inline Edit Demo | `cargo run -p kael_ui --example inline_edit_demo --features kael/runtime_shaders` |
| Input Custom | `cargo run -p kael_ui --example input_custom --features kael/runtime_shaders` |
| Input Demo | `cargo run -p kael_ui --example input_demo --features kael/runtime_shaders` |
| Input Styled Demo | `cargo run -p kael_ui --example input_styled_demo --features kael/runtime_shaders` |
| Input Validation | `cargo run -p kael_ui --example input_validation --features kael/runtime_shaders` |
| Mention Input Demo | `cargo run -p kael_ui --example mention_input_demo --features kael/runtime_shaders` |
| Number Input Demo | `cargo run -p kael_ui --example number_input_demo --features kael/runtime_shaders` |
| Otp Input Demo | `cargo run -p kael_ui --example otp_input_demo --features kael/runtime_shaders` |
| Radio Styled Demo | `cargo run -p kael_ui --example radio_styled_demo --features kael/runtime_shaders` |
| Range Slider Demo | `cargo run -p kael_ui --example range_slider_demo --features kael/runtime_shaders` |
| Search Input Demo | `cargo run -p kael_ui --example search_input_demo --features kael/runtime_shaders` |
| Search Input Styled Demo | `cargo run -p kael_ui --example search_input_styled_demo --features kael/runtime_shaders` |
| Select Styled Demo | `cargo run -p kael_ui --example select_styled_demo --features kael/runtime_shaders` |
| Select Tooltip Demo | `cargo run -p kael_ui --example select_tooltip_demo --features kael/runtime_shaders` |
| Slider Styled Demo | `cargo run -p kael_ui --example slider_styled_demo --features kael/runtime_shaders` |
| Tag Input Demo | `cargo run -p kael_ui --example tag_input_demo --features kael/runtime_shaders` |
| Text Field Styled Demo | `cargo run -p kael_ui --example text_field_styled_demo --features kael/runtime_shaders` |
| Textarea Styled Demo | `cargo run -p kael_ui --example textarea_styled_demo --features kael/runtime_shaders` |
| Time Picker Demo | `cargo run -p kael_ui --example time_picker_demo --features kael/runtime_shaders` |
| Toggle Group Styled Demo | `cargo run -p kael_ui --example toggle_group_styled_demo --features kael/runtime_shaders` |
| Toggle Styled Demo | `cargo run -p kael_ui --example toggle_styled_demo --features kael/runtime_shaders` |

</details>

<details>
<summary><strong>Overlays & dialogs</strong> (22)</summary>

| Example | Command |
|---------|---------|
| Alert Demo | `cargo run -p kael_ui --example alert_demo --features kael/runtime_shaders` |
| Alert Dialog Styled Demo | `cargo run -p kael_ui --example alert_dialog_styled_demo --features kael/runtime_shaders` |
| App Menu Demo | `cargo run -p kael_ui --example app_menu_demo --features kael/runtime_shaders` |
| App Menu Styled Demo | `cargo run -p kael_ui --example app_menu_styled_demo --features kael/runtime_shaders` |
| Bottom Sheet Styled Demo | `cargo run -p kael_ui --example bottom_sheet_styled_demo --features kael/runtime_shaders` |
| Command Palette Demo | `cargo run -p kael_ui --example command_palette_demo --features kael/runtime_shaders` |
| Command Palette Styled Demo | `cargo run -p kael_ui --example command_palette_styled_demo --features kael/runtime_shaders` |
| Confirm Dialog Styled Demo | `cargo run -p kael_ui --example confirm_dialog_styled_demo --features kael/runtime_shaders` |
| Context Menu Styled Demo | `cargo run -p kael_ui --example context_menu_styled_demo --features kael/runtime_shaders` |
| Dialog Styled Demo | `cargo run -p kael_ui --example dialog_styled_demo --features kael/runtime_shaders` |
| Dropdown Demo | `cargo run -p kael_ui --example dropdown_demo --features kael/runtime_shaders` |
| Menu Demo | `cargo run -p kael_ui --example menu_demo --features kael/runtime_shaders` |
| Menu Styled Demo | `cargo run -p kael_ui --example menu_styled_demo --features kael/runtime_shaders` |
| Navigation Menu Demo | `cargo run -p kael_ui --example navigation_menu_demo --features kael/runtime_shaders` |
| Navigation Menu Styled Demo | `cargo run -p kael_ui --example navigation_menu_styled_demo --features kael/runtime_shaders` |
| Notification Center Demo | `cargo run -p kael_ui --example notification_center_demo --features kael/runtime_shaders` |
| Overlays Demo | `cargo run -p kael_ui --example overlays_demo --features kael/runtime_shaders` |
| Popover Menu Styled Demo | `cargo run -p kael_ui --example popover_menu_styled_demo --features kael/runtime_shaders` |
| Popover Styled Demo | `cargo run -p kael_ui --example popover_styled_demo --features kael/runtime_shaders` |
| Sheet Styled Demo | `cargo run -p kael_ui --example sheet_styled_demo --features kael/runtime_shaders` |
| Toast Styled Demo | `cargo run -p kael_ui --example toast_styled_demo --features kael/runtime_shaders` |
| Tooltip Styled Demo | `cargo run -p kael_ui --example tooltip_styled_demo --features kael/runtime_shaders` |

</details>

<details>
<summary><strong>Navigation & layout</strong> (18)</summary>

| Example | Command |
|---------|---------|
| Accordion Demo | `cargo run -p kael_ui --example accordion_demo --features kael/runtime_shaders` |
| Accordion Styled Demo | `cargo run -p kael_ui --example accordion_styled_demo --features kael/runtime_shaders` |
| Carousel Demo | `cargo run -p kael_ui --example carousel_demo --features kael/runtime_shaders` |
| Collapsible Styled Demo | `cargo run -p kael_ui --example collapsible_styled_demo --features kael/runtime_shaders` |
| Complex Layout Demo | `cargo run -p kael_ui --example complex_layout_demo --features kael/runtime_shaders` |
| Keyboard Shortcuts Demo | `cargo run -p kael_ui --example keyboard_shortcuts_demo --features kael/runtime_shaders` |
| Keyboard Shortcuts Styled Demo | `cargo run -p kael_ui --example keyboard_shortcuts_styled_demo --features kael/runtime_shaders` |
| Label Styled Demo | `cargo run -p kael_ui --example label_styled_demo --features kael/runtime_shaders` |
| Layout Demo | `cargo run -p kael_ui --example layout_demo --features kael/runtime_shaders` |
| Resizable Styled Demo | `cargo run -p kael_ui --example resizable_styled_demo --features kael/runtime_shaders` |
| Separator Styled Demo | `cargo run -p kael_ui --example separator_styled_demo --features kael/runtime_shaders` |
| Sidebar Demo | `cargo run -p kael_ui --example sidebar_demo --features kael/runtime_shaders` |
| Sidebar Styled Demo | `cargo run -p kael_ui --example sidebar_styled_demo --features kael/runtime_shaders` |
| Split Pane Demo | `cargo run -p kael_ui --example split_pane_demo --features kael/runtime_shaders` |
| Tabs Demo | `cargo run -p kael_ui --example tabs_demo --features kael/runtime_shaders` |
| Tabs Styled Demo | `cargo run -p kael_ui --example tabs_styled_demo --features kael/runtime_shaders` |
| Toolbar Demo | `cargo run -p kael_ui --example toolbar_demo --features kael/runtime_shaders` |
| Toolbar Styled Demo | `cargo run -p kael_ui --example toolbar_styled_demo --features kael/runtime_shaders` |

</details>

<details>
<summary><strong>Buttons & feedback</strong> (7)</summary>

| Example | Command |
|---------|---------|
| Button Features Demo | `cargo run -p kael_ui --example button_features_demo --features kael/runtime_shaders` |
| Button Styled Demo | `cargo run -p kael_ui --example button_styled_demo --features kael/runtime_shaders` |
| Countdown Demo | `cargo run -p kael_ui --example countdown_demo --features kael/runtime_shaders` |
| Icon Button Styled Demo | `cargo run -p kael_ui --example icon_button_styled_demo --features kael/runtime_shaders` |
| Progress Demo | `cargo run -p kael_ui --example progress_demo --features kael/runtime_shaders` |
| Progress Styled Demo | `cargo run -p kael_ui --example progress_styled_demo --features kael/runtime_shaders` |
| Spinner Demo | `cargo run -p kael_ui --example spinner_demo --features kael/runtime_shaders` |

</details>

<details>
<summary><strong>Motion & interaction</strong> (5)</summary>

| Example | Command |
|---------|---------|
| Animations Demo | `cargo run -p kael_ui --example animations_demo --features kael/runtime_shaders` |
| Drag Drop Demo | `cargo run -p kael_ui --example drag_drop_demo --features kael/runtime_shaders` |
| Drag Drop Styled Demo | `cargo run -p kael_ui --example drag_drop_styled_demo --features kael/runtime_shaders` |
| Drag Spring | `cargo run -p kael_ui --example drag_spring --features kael/runtime_shaders` |
| Transitions Demo | `cargo run -p kael_ui --example transitions_demo --features kael/runtime_shaders` |

</details>

<details>
<summary><strong>Media & text</strong> (7)</summary>

| Example | Command |
|---------|---------|
| Audio Player Demo | `cargo run -p kael_ui --example audio_player_demo --features kael/runtime_shaders` |
| Html Demo | `cargo run -p kael_ui --example html_demo --features kael/runtime_shaders` |
| Icon Showcase | `cargo run -p kael_ui --example icon_showcase --features kael/runtime_shaders` |
| Image Viewer Demo | `cargo run -p kael_ui --example image_viewer_demo --features kael/runtime_shaders` |
| Markdown Demo | `cargo run -p kael_ui --example markdown_demo --features kael/runtime_shaders` |
| Text Demo | `cargo run -p kael_ui --example text_demo --features kael/runtime_shaders` |
| Video Player Demo | `cargo run -p kael_ui --example video_player_demo --features kael/runtime_shaders` |

</details>

<details>
<summary><strong>Showcases</strong> (6)</summary>

| Example | Command |
|---------|---------|
| Async Query | `cargo run -p kael_ui --example async_query --features kael/runtime_shaders` |
| Components Showcase | `cargo run -p kael_ui --example components_showcase --features kael/runtime_shaders` |
| Custom Theme Demo | `cargo run -p kael_ui --example custom_theme_demo --features kael/runtime_shaders` |
| Demo | `cargo run -p kael_ui --example demo --features kael/runtime_shaders` |
| Gpui Extensions Showcase | `cargo run -p kael_ui --example gpui_extensions_showcase --features kael/runtime_shaders` |
| Polish V2 Demo | `cargo run -p kael_ui --example polish_v2_demo --features kael/runtime_shaders` |

</details>

<details>
<summary><strong>Tests & internals</strong> (23)</summary>

| Example | Command |
|---------|---------|
| Click Test | `cargo run -p kael_ui --example click_test --features kael/runtime_shaders` |
| Debug Into Element | `cargo run -p kael_ui --example debug_into_element --features kael/runtime_shaders` |
| Debug Scroll | `cargo run -p kael_ui --example debug_scroll --features kael/runtime_shaders` |
| Demo No Scroll | `cargo run -p kael_ui --example demo_no_scroll --features kael/runtime_shaders` |
| Editor Scroll Test | `cargo run -p kael_ui --example editor_scroll_test --features kael/runtime_shaders` |
| Gpui Scroll | `cargo run -p kael_ui --example gpui-scroll --features kael/runtime_shaders` |
| Icon Path Test | `cargo run -p kael_ui --example icon_path_test --features kael/runtime_shaders` |
| Icon Test | `cargo run -p kael_ui --example icon_test --features kael/runtime_shaders` |
| Input Focus | `cargo run -p kael_ui --example input_focus --features kael/runtime_shaders` |
| Input Test | `cargo run -p kael_ui --example input_test --features kael/runtime_shaders` |
| Mimic Scroll Container | `cargo run -p kael_ui --example mimic_scroll_container --features kael/runtime_shaders` |
| Minimal Button | `cargo run -p kael_ui --example minimal_button --features kael/runtime_shaders` |
| Minimal Scroll Test | `cargo run -p kael_ui --example minimal_scroll_test --features kael/runtime_shaders` |
| Password Test | `cargo run -p kael_ui --example password_test --features kael/runtime_shaders` |
| Scroll Test | `cargo run -p kael_ui --example scroll_test --features kael/runtime_shaders` |
| Simple Button Test | `cargo run -p kael_ui --example simple_button_test --features kael/runtime_shaders` |
| Simple Layout Demo | `cargo run -p kael_ui --example simple_layout_demo --features kael/runtime_shaders` |
| Test Element Id | `cargo run -p kael_ui --example test_element_id --features kael/runtime_shaders` |
| Test Extra Fields | `cargo run -p kael_ui --example test_extra_fields --features kael/runtime_shaders` |
| Test Horizontal Scroll | `cargo run -p kael_ui --example test_horizontal_scroll --features kael/runtime_shaders` |
| Test Real Scrollcontainer | `cargo run -p kael_ui --example test_real_scrollcontainer --features kael/runtime_shaders` |
| Test Scroll Container | `cargo run -p kael_ui --example test_scroll_container --features kael/runtime_shaders` |
| Test Simple Scroll | `cargo run -p kael_ui --example test_simple_scroll --features kael/runtime_shaders` |

</details>

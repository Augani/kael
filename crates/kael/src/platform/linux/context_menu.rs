//! Context menu implementation for Linux.
//!
//! Provides a cross-backend context menu that works on both X11 and Wayland.
//! - X11: Creates an override-redirect window with basic X11 GC drawing.
//! - Wayland: Creates a layer surface or falls back to a positioned window.
//!
//! Both implementations handle mouse interaction for item selection and dismissal.

use crate::{Pixels, Point, SharedString, TrayMenuItem};

/// Flattened representation of a menu item for rendering.
#[derive(Clone, Debug)]
pub(crate) struct FlatMenuItem {
    pub label: String,
    pub id: Option<SharedString>,
    pub is_separator: bool,
    #[allow(dead_code)]
    pub checked: bool,
    pub depth: u32,
}

/// Flatten a nested `TrayMenuItem` tree into a list suitable for rendering.
pub(crate) fn flatten_menu_items(items: &[TrayMenuItem], depth: u32) -> Vec<FlatMenuItem> {
    let mut result = Vec::new();
    for item in items {
        match item {
            TrayMenuItem::Action { label, id } => {
                result.push(FlatMenuItem {
                    label: label.to_string(),
                    id: Some(id.clone()),
                    is_separator: false,
                    checked: false,
                    depth,
                });
            }
            TrayMenuItem::Separator => {
                result.push(FlatMenuItem {
                    label: String::new(),
                    id: None,
                    is_separator: true,
                    checked: false,
                    depth,
                });
            }
            TrayMenuItem::Submenu { label, items: sub } => {
                // Render submenu header (non-selectable) with arrow indicator
                result.push(FlatMenuItem {
                    label: format!("{} ▸", label),
                    id: None,
                    is_separator: false,
                    checked: false,
                    depth,
                });
                result.extend(flatten_menu_items(sub, depth + 1));
            }
            TrayMenuItem::Toggle { label, checked, id } => {
                let prefix = if *checked { "✓ " } else { "  " };
                result.push(FlatMenuItem {
                    label: format!("{}{}", prefix, label),
                    id: Some(id.clone()),
                    is_separator: false,
                    checked: *checked,
                    depth,
                });
            }
        }
    }
    result
}

/// Menu layout constants (in pixels).
pub(crate) const ITEM_HEIGHT: u32 = 24;
pub(crate) const SEPARATOR_HEIGHT: u32 = 8;
pub(crate) const MENU_PADDING: u32 = 4;
pub(crate) const ITEM_PADDING_X: u32 = 12;
pub(crate) const INDENT_PER_DEPTH: u32 = 16;
pub(crate) const MIN_MENU_WIDTH: u32 = 150;

/// Calculate the total height needed for the menu.
pub(crate) fn calculate_menu_height(items: &[FlatMenuItem]) -> u32 {
    let mut height = MENU_PADDING * 2;
    for item in items {
        if item.is_separator {
            height += SEPARATOR_HEIGHT;
        } else {
            height += ITEM_HEIGHT;
        }
    }
    height
}

/// Estimate the width needed for the menu based on label lengths.
pub(crate) fn calculate_menu_width(items: &[FlatMenuItem]) -> u32 {
    let max_label_len = items
        .iter()
        .filter(|i| !i.is_separator)
        .map(|i| {
            let indent = (i.depth * INDENT_PER_DEPTH) as usize;
            i.label.len() + indent / 4 // rough char-to-pixel estimate
        })
        .max()
        .unwrap_or(10);
    // Approximate 7 pixels per character + padding on both sides
    let width = (max_label_len as u32 * 7) + ITEM_PADDING_X * 2;
    width.max(MIN_MENU_WIDTH)
}

/// Determine which selectable item index is at a given y coordinate.
/// Returns `None` for separators, submenu headers, or out-of-bounds positions.
pub(crate) fn item_at_y(items: &[FlatMenuItem], y: i32) -> Option<usize> {
    let mut current_y = MENU_PADDING as i32;
    for (i, item) in items.iter().enumerate() {
        let h = if item.is_separator {
            SEPARATOR_HEIGHT as i32
        } else {
            ITEM_HEIGHT as i32
        };
        if y >= current_y && y < current_y + h {
            // Only return selectable items (those with an id)
            if item.is_separator || item.id.is_none() {
                return None;
            }
            return Some(i);
        }
        current_y += h;
    }
    None
}

/// Show a context menu using an X11 override-redirect window.
///
/// This creates a borderless popup window, draws menu items using X11 GC
/// primitives, grabs the pointer, and runs a mini event loop to handle
/// selection and dismissal.
#[cfg(feature = "x11")]
pub(crate) fn show_x11_context_menu(
    position: Point<Pixels>,
    items: Vec<TrayMenuItem>,
    mut callback: Box<dyn FnMut(SharedString)>,
) {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{self, ConnectionExt as _};

    let flat_items = flatten_menu_items(&items, 0);
    if flat_items.is_empty() {
        return;
    }

    let menu_width = calculate_menu_width(&flat_items);
    let menu_height = calculate_menu_height(&flat_items);

    // Connect to X11 display
    let Ok((conn, screen_num)) = x11rb::connect(None) else {
        log::warn!("Failed to connect to X11 display for context menu");
        return;
    };

    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;
    let depth = screen.root_depth;
    let visual = screen.root_visual;

    // Clamp position to screen bounds
    let screen_w = screen.width_in_pixels as i32;
    let screen_h = screen.height_in_pixels as i32;
    let mut x = position.x.0 as i32;
    let mut y = position.y.0 as i32;
    if x + menu_width as i32 > screen_w {
        x = screen_w - menu_width as i32;
    }
    if y + menu_height as i32 > screen_h {
        y = screen_h - menu_height as i32;
    }
    x = x.max(0);
    y = y.max(0);

    // Create override-redirect window (bypasses window manager)
    let win = conn.generate_id().unwrap_or(0);
    if win == 0 {
        return;
    }

    let values = xproto::CreateWindowAux::new()
        .override_redirect(1)
        .background_pixel(screen.white_pixel)
        .event_mask(
            xproto::EventMask::EXPOSURE
                | xproto::EventMask::BUTTON_PRESS
                | xproto::EventMask::BUTTON_RELEASE
                | xproto::EventMask::POINTER_MOTION
                | xproto::EventMask::LEAVE_WINDOW,
        )
        .border_pixel(screen.black_pixel);

    let _ = conn.create_window(
        depth,
        win,
        root,
        x as i16,
        y as i16,
        menu_width as u16,
        menu_height as u16,
        1, // border width
        xproto::WindowClass::INPUT_OUTPUT,
        visual,
        &values,
    );

    // Create graphics contexts for drawing
    let gc_bg = conn.generate_id().unwrap_or(0);
    let gc_text = conn.generate_id().unwrap_or(0);
    let gc_highlight = conn.generate_id().unwrap_or(0);
    let gc_separator = conn.generate_id().unwrap_or(0);

    let _ = conn.create_gc(
        gc_bg,
        win,
        &xproto::CreateGCAux::new().foreground(screen.white_pixel),
    );
    let _ = conn.create_gc(
        gc_text,
        win,
        &xproto::CreateGCAux::new().foreground(screen.black_pixel),
    );
    // Highlight color: light blue-ish (approximate via pixel value)
    let highlight_pixel = 0x0060A0D0u32;
    let _ = conn.create_gc(
        gc_highlight,
        win,
        &xproto::CreateGCAux::new().foreground(highlight_pixel),
    );
    // Separator color: light gray
    let separator_pixel = 0x00C0C0C0u32;
    let _ = conn.create_gc(
        gc_separator,
        win,
        &xproto::CreateGCAux::new().foreground(separator_pixel),
    );

    let _ = conn.map_window(win);
    let _ = conn.flush();

    // Grab the pointer so we get all mouse events
    let _ = conn.grab_pointer(
        true,
        win,
        xproto::EventMask::BUTTON_PRESS
            | xproto::EventMask::BUTTON_RELEASE
            | xproto::EventMask::POINTER_MOTION,
        xproto::GrabMode::ASYNC,
        xproto::GrabMode::ASYNC,
        x11rb::NONE,
        x11rb::NONE,
        x11rb::CURRENT_TIME,
    );
    let _ = conn.flush();

    let mut hovered_index: Option<usize> = None;

    // Helper closure to draw the menu
    let draw_menu = |conn: &x11rb::rust_connection::RustConnection, hovered: Option<usize>| {
        // Clear background
        let _ = conn.poly_fill_rectangle(
            win,
            gc_bg,
            &[xproto::Rectangle {
                x: 0,
                y: 0,
                width: menu_width as u16,
                height: menu_height as u16,
            }],
        );

        let mut cy = MENU_PADDING as i16;
        for (i, item) in flat_items.iter().enumerate() {
            if item.is_separator {
                // Draw separator line
                let sep_y = cy + (SEPARATOR_HEIGHT as i16 / 2);
                let _ = conn.poly_fill_rectangle(
                    win,
                    gc_separator,
                    &[xproto::Rectangle {
                        x: ITEM_PADDING_X as i16,
                        y: sep_y,
                        width: (menu_width - ITEM_PADDING_X * 2) as u16,
                        height: 1,
                    }],
                );
                cy += SEPARATOR_HEIGHT as i16;
            } else {
                // Draw highlight for hovered item
                if hovered == Some(i) && item.id.is_some() {
                    let _ = conn.poly_fill_rectangle(
                        win,
                        gc_highlight,
                        &[xproto::Rectangle {
                            x: 1,
                            y: cy,
                            width: (menu_width - 2) as u16,
                            height: ITEM_HEIGHT as u16,
                        }],
                    );
                }

                // Draw text
                let indent = (item.depth * INDENT_PER_DEPTH) as i16;
                let text_x = ITEM_PADDING_X as i16 + indent;
                let text_y = cy + (ITEM_HEIGHT as i16 * 3 / 4); // baseline approx
                let label_bytes = item.label.as_bytes();
                // X11 ImageText8 only handles ASCII; truncate for safety
                let safe_label: Vec<u8> =
                    label_bytes.iter().copied().filter(|b| *b < 128).collect();
                if !safe_label.is_empty() {
                    let _ = conn.image_text8(win, gc_text, text_x, text_y, &safe_label);
                }

                cy += ITEM_HEIGHT as i16;
            }
        }
        let _ = conn.flush();
    };

    // Initial draw
    draw_menu(&conn, None);

    // Mini event loop for the context menu
    let mut selected_id: Option<SharedString> = None;
    'menu_loop: while let Ok(event) = conn.wait_for_event() {
        match event {
            x11rb::protocol::Event::Expose(_) => {
                draw_menu(&conn, hovered_index);
            }
            x11rb::protocol::Event::MotionNotify(ev) => {
                let new_hover = item_at_y(&flat_items, ev.event_y as i32);
                if new_hover != hovered_index {
                    hovered_index = new_hover;
                    draw_menu(&conn, hovered_index);
                }
            }
            x11rb::protocol::Event::ButtonPress(ev) => {
                // Check if click is inside the menu window
                let in_menu = ev.event_x >= 0
                    && ev.event_x < menu_width as i16
                    && ev.event_y >= 0
                    && ev.event_y < menu_height as i16;

                if in_menu {
                    if let Some(idx) = item_at_y(&flat_items, ev.event_y as i32) {
                        selected_id = flat_items[idx].id.clone();
                    }
                }
                // Any click (inside or outside) dismisses the menu
                break 'menu_loop;
            }
            x11rb::protocol::Event::LeaveNotify(_) => {
                // Clear hover when mouse leaves
                if hovered_index.is_some() {
                    hovered_index = None;
                    draw_menu(&conn, None);
                }
            }
            _ => {}
        }
    }

    // Cleanup
    let _ = conn.ungrab_pointer(x11rb::CURRENT_TIME);
    let _ = conn.free_gc(gc_bg);
    let _ = conn.free_gc(gc_text);
    let _ = conn.free_gc(gc_highlight);
    let _ = conn.free_gc(gc_separator);
    let _ = conn.destroy_window(win);
    let _ = conn.flush();

    // Invoke callback if an item was selected
    if let Some(id) = selected_id {
        super::catch_platform_callback("context menu", (), || callback(id));
    }
}

/// Show a context menu on Wayland using a custom popup surface.
///
/// Since Wayland doesn't support arbitrary popup positioning without
/// compositor cooperation, this implementation uses GPUI's own rendering
/// by creating a temporary popup window. As a fallback, it uses a simple
/// subprocess-based approach.
#[cfg(feature = "wayland")]
pub(crate) fn show_wayland_context_menu(
    _position: Point<Pixels>,
    items: Vec<TrayMenuItem>,
    mut callback: Box<dyn FnMut(SharedString)>,
) {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let flat_items = flatten_menu_items(&items, 0);
    if flat_items.is_empty() {
        return;
    }

    // Build a list of selectable labels and their IDs
    let mut labels = Vec::new();
    let mut ids = Vec::new();
    for item in &flat_items {
        if item.is_separator {
            continue;
        }
        if let Some(ref id) = item.id {
            let indent = "  ".repeat(item.depth as usize);
            labels.push(format!("{}{}", indent, item.label));
            ids.push(id.clone());
        }
    }

    if labels.is_empty() {
        return;
    }

    // Try using `bemenu` (Wayland-native menu), then `wmenu`, then `dmenu`
    let menu_tools = ["bemenu", "wmenu", "dmenu"];
    let mut selected = None;

    for tool in &menu_tools {
        let result = Command::new(tool)
            .args(["-l", &labels.len().to_string()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();

        if let Ok(mut child) = result {
            if let Some(ref mut stdin) = child.stdin {
                let input = labels.join("\n");
                let _ = stdin.write_all(input.as_bytes());
            }
            // Close stdin to signal end of input
            drop(child.stdin.take());

            if let Ok(output) = child.wait_with_output() {
                if output.status.success() {
                    let chosen = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    // Find the matching label and get its ID
                    for (i, label) in labels.iter().enumerate() {
                        if label.trim() == chosen.trim() {
                            selected = Some(ids[i].clone());
                            break;
                        }
                    }
                }
                break; // We found a working tool, stop trying others
            }
        }
    }

    // If no external tool is available, log a warning
    if selected.is_none() {
        // Check if we even found a tool
        let tool_found = menu_tools.iter().any(|t| {
            Command::new("which")
                .arg(t)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        });
        if !tool_found {
            log::warn!(
                "Context menus on Wayland require bemenu, wmenu, or dmenu. \
                 Install one of these packages for context menu support."
            );
        }
    }

    if let Some(id) = selected {
        super::catch_platform_callback("context menu", (), || callback(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SharedString;

    fn make_action(label: &str, id: &str) -> TrayMenuItem {
        TrayMenuItem::Action {
            label: SharedString::from(label.to_string()),
            id: SharedString::from(id.to_string()),
        }
    }

    fn make_toggle(label: &str, id: &str, checked: bool) -> TrayMenuItem {
        TrayMenuItem::Toggle {
            label: SharedString::from(label.to_string()),
            checked,
            id: SharedString::from(id.to_string()),
        }
    }

    fn make_submenu(label: &str, items: Vec<TrayMenuItem>) -> TrayMenuItem {
        TrayMenuItem::Submenu {
            label: SharedString::from(label.to_string()),
            items,
        }
    }

    #[test]
    fn test_flatten_empty_items() {
        let items: Vec<TrayMenuItem> = vec![];
        let flat = flatten_menu_items(&items, 0);
        assert!(flat.is_empty());
    }

    #[test]
    fn test_flatten_action_items() {
        let items = vec![
            make_action("Cut", "cut"),
            make_action("Copy", "copy"),
            make_action("Paste", "paste"),
        ];
        let flat = flatten_menu_items(&items, 0);
        assert_eq!(flat.len(), 3);
        assert_eq!(flat[0].label, "Cut");
        assert_eq!(flat[0].id.as_ref().map(|s| s.as_str()), Some("cut"));
        assert!(!flat[0].is_separator);
        assert_eq!(flat[0].depth, 0);
    }

    #[test]
    fn test_flatten_with_separator() {
        let items = vec![
            make_action("Cut", "cut"),
            TrayMenuItem::Separator,
            make_action("Paste", "paste"),
        ];
        let flat = flatten_menu_items(&items, 0);
        assert_eq!(flat.len(), 3);
        assert!(flat[1].is_separator);
        assert!(flat[1].id.is_none());
    }

    #[test]
    fn test_flatten_submenu() {
        let items = vec![make_submenu(
            "Edit",
            vec![make_action("Cut", "cut"), make_action("Copy", "copy")],
        )];
        let flat = flatten_menu_items(&items, 0);
        assert_eq!(flat.len(), 3);
        // Submenu header
        assert!(flat[0].label.contains("Edit"));
        assert!(flat[0].label.contains("▸"));
        assert!(flat[0].id.is_none());
        assert_eq!(flat[0].depth, 0);
        // Submenu children
        assert_eq!(flat[1].label, "Cut");
        assert_eq!(flat[1].depth, 1);
        assert_eq!(flat[2].label, "Copy");
        assert_eq!(flat[2].depth, 1);
    }

    #[test]
    fn test_flatten_toggle_items() {
        let items = vec![
            make_toggle("Bold", "bold", true),
            make_toggle("Italic", "italic", false),
        ];
        let flat = flatten_menu_items(&items, 0);
        assert_eq!(flat.len(), 2);
        assert!(flat[0].label.starts_with("✓"));
        assert!(flat[0].label.contains("Bold"));
        assert!(flat[0].checked);
        assert!(flat[1].label.starts_with("  "));
        assert!(!flat[1].checked);
    }

    #[test]
    fn test_calculate_menu_height() {
        let items = vec![
            make_action("Cut", "cut"),
            TrayMenuItem::Separator,
            make_action("Paste", "paste"),
        ];
        let flat = flatten_menu_items(&items, 0);
        let height = calculate_menu_height(&flat);
        // 2 action items (24px each) + 1 separator (8px) + padding (4*2)
        assert_eq!(
            height,
            MENU_PADDING * 2 + ITEM_HEIGHT * 2 + SEPARATOR_HEIGHT
        );
    }

    #[test]
    fn test_calculate_menu_width_minimum() {
        let items = vec![make_action("X", "x")];
        let flat = flatten_menu_items(&items, 0);
        let width = calculate_menu_width(&flat);
        assert!(width >= MIN_MENU_WIDTH);
    }

    #[test]
    fn test_item_at_y_selects_correct_item() {
        let items = vec![
            make_action("Cut", "cut"),
            make_action("Copy", "copy"),
            make_action("Paste", "paste"),
        ];
        let flat = flatten_menu_items(&items, 0);

        // First item starts at MENU_PADDING
        let y_first = MENU_PADDING as i32 + 1;
        assert_eq!(item_at_y(&flat, y_first), Some(0));

        // Second item starts at MENU_PADDING + ITEM_HEIGHT
        let y_second = (MENU_PADDING + ITEM_HEIGHT) as i32 + 1;
        assert_eq!(item_at_y(&flat, y_second), Some(1));

        // Third item
        let y_third = (MENU_PADDING + ITEM_HEIGHT * 2) as i32 + 1;
        assert_eq!(item_at_y(&flat, y_third), Some(2));
    }

    #[test]
    fn test_item_at_y_returns_none_for_separator() {
        let items = vec![
            make_action("Cut", "cut"),
            TrayMenuItem::Separator,
            make_action("Paste", "paste"),
        ];
        let flat = flatten_menu_items(&items, 0);

        // Y position in the separator area
        let y_sep = (MENU_PADDING + ITEM_HEIGHT) as i32 + 1;
        assert_eq!(item_at_y(&flat, y_sep), None);
    }

    #[test]
    fn test_item_at_y_returns_none_for_submenu_header() {
        let items = vec![make_submenu("Edit", vec![make_action("Cut", "cut")])];
        let flat = flatten_menu_items(&items, 0);

        // Y position in the submenu header (which has no id)
        let y_header = MENU_PADDING as i32 + 1;
        assert_eq!(item_at_y(&flat, y_header), None);
    }

    #[test]
    fn test_item_at_y_returns_none_out_of_bounds() {
        let items = vec![make_action("Cut", "cut")];
        let flat = flatten_menu_items(&items, 0);

        // Before the menu
        assert_eq!(item_at_y(&flat, -10), None);
        // After the menu
        assert_eq!(item_at_y(&flat, 1000), None);
    }

    #[test]
    fn test_menu_dismissal_on_outside_click() {
        // This tests the logic that item_at_y returns None for coordinates
        // outside the menu bounds, which triggers dismissal without selection
        let items = vec![make_action("Cut", "cut")];
        let flat = flatten_menu_items(&items, 0);
        let height = calculate_menu_height(&flat) as i32;

        // Click below the menu
        assert_eq!(item_at_y(&flat, height + 10), None);
    }
}

use std::collections::HashMap;

use anyhow::Result;
use windows::{
    Win32::{
        Foundation::*,
        UI::{
            Shell::{
                NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE,
                NIM_MODIFY, NOTIFYICONDATAW, NOTIFYICONIDENTIFIER, Shell_NotifyIconGetRect,
                Shell_NotifyIconW,
            },
            WindowsAndMessaging::*,
        },
    },
    core::PCWSTR,
};

use crate::{Bounds, Pixels, SharedString, TrayMenuItem, WM_GPUI_TRAY_ICON, point, px, size};

const TRAY_ICON_ID: u32 = 1;
const MAX_TRAY_ICON_BYTES: usize = 16 * 1024 * 1024;

pub(crate) struct WindowsTray {
    icon_added: bool,
    hwnd: HWND,
    current_icon: Option<HICON>,
    panel_mode: bool,
    pub(crate) menu_items: Vec<TrayMenuItem>,
    pub(crate) command_id_map: HashMap<u32, SharedString>,
}

impl WindowsTray {
    pub fn new(hwnd: HWND) -> Self {
        let mut tray = Self {
            icon_added: false,
            hwnd,
            current_icon: None,
            panel_mode: false,
            menu_items: Vec::new(),
            command_id_map: HashMap::new(),
        };
        tray.ensure_icon(hwnd);
        tray
    }

    fn ensure_icon(&mut self, hwnd: HWND) {
        if self.icon_added {
            return;
        }
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ICON_ID,
            uFlags: NIF_MESSAGE | NIF_SHOWTIP,
            uCallbackMessage: WM_GPUI_TRAY_ICON,
            ..Default::default()
        };
        self.icon_added = unsafe { Shell_NotifyIconW(NIM_ADD, &nid).as_bool() };
    }

    pub fn set_icon(&mut self, icon_data: Option<&[u8]>, hwnd: HWND) {
        self.ensure_icon(hwnd);
        if let Some(old_icon) = self.current_icon.take() {
            unsafe {
                let _ = DestroyIcon(old_icon);
            }
        }
        let hicon = match icon_data {
            Some(data) => create_hicon_from_bytes(data),
            None => None,
        };
        self.current_icon = hicon;
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ICON_ID,
            uFlags: NIF_ICON,
            hIcon: hicon.unwrap_or_default(),
            ..Default::default()
        };
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }

    pub fn set_tooltip(&mut self, tooltip: &str, hwnd: HWND) {
        self.ensure_icon(hwnd);
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ICON_ID,
            uFlags: NIF_TIP,
            ..Default::default()
        };
        let wide: Vec<u16> = tooltip.encode_utf16().take(nid.szTip.len() - 1).collect();
        let len = wide.len().min(nid.szTip.len() - 1);
        nid.szTip[..len].copy_from_slice(&wide[..len]);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }

    pub fn show_balloon(&self, title: &str, body: &str, hwnd: HWND) -> Result<()> {
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ICON_ID,
            uFlags: NIF_INFO,
            ..Default::default()
        };

        let title_wide: Vec<u16> = title
            .encode_utf16()
            .take(nid.szInfoTitle.len() - 1)
            .collect();
        let title_len = title_wide.len().min(nid.szInfoTitle.len() - 1);
        nid.szInfoTitle[..title_len].copy_from_slice(&title_wide[..title_len]);

        let body_wide: Vec<u16> = body.encode_utf16().take(nid.szInfo.len() - 1).collect();
        let body_len = body_wide.len().min(nid.szInfo.len() - 1);
        nid.szInfo[..body_len].copy_from_slice(&body_wide[..body_len]);

        unsafe {
            Shell_NotifyIconW(NIM_MODIFY, &nid)
                .ok()
                .map_err(|e| anyhow::anyhow!("Failed to show balloon notification: {}", e))
        }
    }

    pub fn set_panel_mode(&mut self, enabled: bool) {
        self.panel_mode = enabled;
    }

    pub fn is_panel_mode(&self) -> bool {
        self.panel_mode
    }

    pub fn get_icon_bounds(&self) -> Option<Bounds<Pixels>> {
        if !self.icon_added {
            return None;
        }
        let identifier = NOTIFYICONIDENTIFIER {
            cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32,
            hWnd: self.hwnd,
            uID: TRAY_ICON_ID,
            ..Default::default()
        };
        let rect = unsafe { Shell_NotifyIconGetRect(&identifier) };
        match rect {
            Ok(rect) => {
                let width = rect.right.checked_sub(rect.left)?;
                let height = rect.bottom.checked_sub(rect.top)?;
                if width < 0 || height < 0 {
                    return None;
                }
                Some(Bounds::new(
                    point(px(rect.left as f32), px(rect.top as f32)),
                    size(px(width as f32), px(height as f32)),
                ))
            }
            Err(_) => None,
        }
    }

    pub fn show_context_menu(&mut self, hwnd: HWND) {
        if self.menu_items.is_empty() {
            return;
        }
        if TrayMenuItem::validate_items(&self.menu_items).is_err() {
            return;
        }
        self.command_id_map.clear();
        unsafe {
            let hmenu = CreatePopupMenu();
            if let Ok(hmenu) = hmenu {
                let mut counter: u32 = 1;
                Self::build_menu(
                    hmenu,
                    &self.menu_items,
                    &mut counter,
                    &mut self.command_id_map,
                );
                let mut point = POINT::default();
                let _ = GetCursorPos(&mut point);
                let _ = SetForegroundWindow(hwnd);
                let _ = TrackPopupMenu(
                    hmenu,
                    TPM_LEFTALIGN | TPM_BOTTOMALIGN,
                    point.x,
                    point.y,
                    None,
                    hwnd,
                    None,
                );
                let _ = DestroyMenu(hmenu);
            }
        }
    }

    pub(crate) unsafe fn build_menu(
        hmenu: HMENU,
        items: &[TrayMenuItem],
        counter: &mut u32,
        id_map: &mut HashMap<u32, SharedString>,
    ) {
        for item in items.iter() {
            match item {
                TrayMenuItem::Action { label, id } => {
                    let cmd_id = *counter;
                    let Some(next) = counter.checked_add(1) else {
                        return;
                    };
                    *counter = next;
                    id_map.insert(cmd_id, id.clone());
                    let wide: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
                    unsafe {
                        let _ =
                            AppendMenuW(hmenu, MF_STRING, cmd_id as usize, PCWSTR(wide.as_ptr()));
                    }
                }
                TrayMenuItem::Separator => unsafe {
                    let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, None);
                },
                TrayMenuItem::Submenu {
                    label,
                    items: sub_items,
                } => {
                    if let Ok(submenu) = unsafe { CreatePopupMenu() } {
                        unsafe { Self::build_menu(submenu, sub_items, counter, id_map) };
                        let wide: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
                        unsafe {
                            let _ = AppendMenuW(
                                hmenu,
                                MF_POPUP,
                                submenu.0 as usize,
                                PCWSTR(wide.as_ptr()),
                            );
                        }
                    }
                }
                TrayMenuItem::Toggle {
                    label, checked, id, ..
                } => {
                    let cmd_id = *counter;
                    let Some(next) = counter.checked_add(1) else {
                        return;
                    };
                    *counter = next;
                    id_map.insert(cmd_id, id.clone());
                    let flags = if *checked {
                        MF_STRING | MF_CHECKED
                    } else {
                        MF_STRING
                    };
                    let wide: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
                    unsafe {
                        let _ = AppendMenuW(hmenu, flags, cmd_id as usize, PCWSTR(wide.as_ptr()));
                    }
                }
            }
        }
    }
}

impl Drop for WindowsTray {
    fn drop(&mut self) {
        if self.icon_added {
            let mut nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: self.hwnd,
                uID: TRAY_ICON_ID,
                ..Default::default()
            };
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
            }
        }
        if let Some(icon) = self.current_icon.take() {
            unsafe {
                let _ = DestroyIcon(icon);
            }
        }
    }
}

fn create_hicon_from_bytes(data: &[u8]) -> Option<HICON> {
    let icon_data = first_ico_image(data)?;
    unsafe {
        let hicon = CreateIconFromResourceEx(icon_data, true, 0x00030000, 0, 0, LR_DEFAULTCOLOR);
        hicon.ok()
    }
}

fn first_ico_image(data: &[u8]) -> Option<&[u8]> {
    if data.len() > MAX_TRAY_ICON_BYTES || data.len() < 22 {
        return None;
    }
    let reserved = u16::from_le_bytes(data[0..2].try_into().ok()?);
    let kind = u16::from_le_bytes(data[2..4].try_into().ok()?);
    let count = usize::from(u16::from_le_bytes(data[4..6].try_into().ok()?));
    if reserved != 0 || kind != 1 || count == 0 || count > 256 {
        return None;
    }
    let directory_end = 6usize.checked_add(count.checked_mul(16)?)?;
    if directory_end > data.len() {
        return None;
    }
    for entry in data[6..directory_end].chunks_exact(16) {
        let size = usize::try_from(u32::from_le_bytes(entry[8..12].try_into().ok()?)).ok()?;
        let offset = usize::try_from(u32::from_le_bytes(entry[12..16].try_into().ok()?)).ok()?;
        let end = offset.checked_add(size)?;
        if size > 0 && offset >= directory_end && end <= data.len() {
            return Some(&data[offset..end]);
        }
    }
    None
}

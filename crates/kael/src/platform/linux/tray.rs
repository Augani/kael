#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use ksni::blocking::TrayMethods as _;

use crate::platform::TrayMenuItem;
use crate::{Bounds, Pixels, SharedString, TrayIconEvent, point, px, size};

type TrayActionCallback = Arc<Mutex<Option<Box<dyn Fn(SharedString) + Send>>>>;
type TrayClickCallback = Arc<Mutex<Option<Box<dyn Fn(TrayIconEvent) + Send>>>>;
type LastClickPosition = Arc<Mutex<Option<(i32, i32)>>>;

fn invoke_click_callback(callbacks: &TrayClickCallback, event: TrayIconEvent) {
    let callback = callbacks.lock().ok().and_then(|mut guard| guard.take());
    if let Some(callback) = callback {
        super::catch_platform_callback("tray icon", (), || callback(event));
        if let Ok(mut guard) = callbacks.lock()
            && guard.is_none()
        {
            *guard = Some(callback);
        }
    }
}

fn invoke_action_callback(callbacks: &TrayActionCallback, id: SharedString) {
    let callback = callbacks.lock().ok().and_then(|mut guard| guard.take());
    if let Some(callback) = callback {
        super::catch_platform_callback("tray menu", (), || callback(id));
        if let Ok(mut guard) = callbacks.lock()
            && guard.is_none()
        {
            *guard = Some(callback);
        }
    }
}

/// Default assumed size for a tray icon on Linux panels.
const TRAY_ICON_SIZE: i32 = 24;

struct GpuiTray {
    icon_data: Vec<u8>,
    tooltip: String,
    menu_items: Vec<TrayMenuItem>,
    action_callback: TrayActionCallback,
    click_callback: TrayClickCallback,
    last_click_position: LastClickPosition,
}

impl ksni::Tray for GpuiTray {
    fn id(&self) -> String {
        "kael".to_string()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        if self.icon_data.is_empty() {
            return vec![];
        }
        if let Ok(img) = image::load_from_memory(&self.icon_data) {
            let rgba = img.to_rgba8();
            let width = rgba.width() as i32;
            let height = rgba.height() as i32;
            let raw = rgba.as_raw();
            let mut argb_data = Vec::with_capacity(raw.len());
            for pixel in raw.chunks_exact(4) {
                argb_data.push(pixel[3]);
                argb_data.push(pixel[0]);
                argb_data.push(pixel[1]);
                argb_data.push(pixel[2]);
            }
            vec![ksni::Icon {
                width,
                height,
                data: argb_data,
            }]
        } else {
            vec![]
        }
    }

    fn title(&self) -> String {
        self.tooltip.clone()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: self.tooltip.clone(),
            ..Default::default()
        }
    }

    fn activate(&mut self, x: i32, y: i32) {
        if let Ok(mut guard) = self.last_click_position.lock() {
            *guard = Some((x, y));
        }
        invoke_click_callback(&self.click_callback, TrayIconEvent::LeftClick);
    }

    fn secondary_activate(&mut self, x: i32, y: i32) {
        if let Ok(mut guard) = self.last_click_position.lock() {
            *guard = Some((x, y));
        }
        invoke_click_callback(&self.click_callback, TrayIconEvent::RightClick);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        self.menu_items
            .iter()
            .map(|item| convert_menu_item(item, &self.action_callback))
            .collect()
    }
}

fn convert_menu_item(
    item: &TrayMenuItem,
    action_callback: &TrayActionCallback,
) -> ksni::MenuItem<GpuiTray> {
    match item {
        TrayMenuItem::Action { label, id } => {
            let id_clone = id.clone();
            let cb = action_callback.clone();
            ksni::MenuItem::Standard(ksni::menu::StandardItem {
                label: label.to_string(),
                activate: Box::new(move |_tray: &mut GpuiTray| {
                    invoke_action_callback(&cb, id_clone.clone());
                }),
                ..Default::default()
            })
        }
        TrayMenuItem::Separator => ksni::MenuItem::Separator,
        TrayMenuItem::Submenu { label, items } => ksni::MenuItem::SubMenu(ksni::menu::SubMenu {
            label: label.to_string(),
            submenu: items
                .iter()
                .map(|i| convert_menu_item(i, action_callback))
                .collect(),
            ..Default::default()
        }),
        TrayMenuItem::Toggle { label, checked, id } => {
            let id_clone = id.clone();
            let cb = action_callback.clone();
            ksni::MenuItem::Standard(ksni::menu::StandardItem {
                label: label.to_string(),
                icon_name: if *checked {
                    "checkbox-checked-symbolic".to_string()
                } else {
                    String::new()
                },
                activate: Box::new(move |_tray: &mut GpuiTray| {
                    invoke_action_callback(&cb, id_clone.clone());
                }),
                ..Default::default()
            })
        }
    }
}

pub struct LinuxTray {
    handle: Option<ksni::blocking::Handle<GpuiTray>>,
    action_callback: TrayActionCallback,
    click_callback: TrayClickCallback,
    panel_mode: bool,
    last_click_position: LastClickPosition,
}

impl LinuxTray {
    pub fn new() -> Self {
        Self {
            handle: None,
            action_callback: Arc::new(Mutex::new(None)),
            click_callback: Arc::new(Mutex::new(None)),
            panel_mode: false,
            last_click_position: Arc::new(Mutex::new(None)),
        }
    }

    fn ensure_started(&mut self) {
        if self.handle.is_some() {
            return;
        }
        let tray = GpuiTray {
            icon_data: Vec::new(),
            tooltip: String::new(),
            menu_items: Vec::new(),
            action_callback: self.action_callback.clone(),
            click_callback: self.click_callback.clone(),
            last_click_position: self.last_click_position.clone(),
        };
        match tray.assume_sni_available(true).spawn() {
            Ok(handle) => self.handle = Some(handle),
            Err(error) => log::error!("failed to start Linux tray service: {error}"),
        }
    }

    pub fn set_icon(&mut self, icon_data: Option<&[u8]>) {
        self.ensure_started();
        if let Some(handle) = &self.handle {
            let data = icon_data.unwrap_or(&[]).to_vec();
            handle.update(move |tray: &mut GpuiTray| {
                tray.icon_data = data;
            });
        }
    }

    pub fn set_tooltip(&mut self, tooltip: &str) {
        self.ensure_started();
        if let Some(handle) = &self.handle {
            let tooltip = tooltip.to_string();
            handle.update(move |tray: &mut GpuiTray| {
                tray.tooltip = tooltip;
            });
        }
    }

    pub fn set_menu(&mut self, items: Vec<TrayMenuItem>) {
        self.ensure_started();
        if let Some(handle) = &self.handle {
            handle.update(move |tray: &mut GpuiTray| {
                tray.menu_items = items;
            });
        }
    }

    pub fn set_on_menu_action(&self, callback: Box<dyn Fn(SharedString) + Send>) {
        if let Ok(mut guard) = self.action_callback.lock() {
            *guard = Some(callback);
        }
    }

    pub fn set_on_click(&self, callback: Box<dyn Fn(TrayIconEvent) + Send>) {
        if let Ok(mut guard) = self.click_callback.lock() {
            *guard = Some(callback);
        }
    }

    pub fn set_panel_mode(&mut self, enabled: bool) {
        self.ensure_started();
        self.panel_mode = enabled;
    }

    pub fn is_panel_mode(&self) -> bool {
        self.panel_mode
    }

    /// Returns the estimated screen bounds of the tray icon.
    ///
    /// On Linux, the StatusNotifierItem D-Bus interface does not expose icon
    /// geometry directly. Instead, we use the `(x, y)` coordinates provided
    /// by the panel/host in the `Activate` and `SecondaryActivate` D-Bus
    /// method calls. These coordinates represent the screen position where
    /// the user clicked the tray icon. We construct an approximate bounding
    /// rectangle centered on that position using a standard icon size.
    pub fn get_icon_bounds(&self) -> Option<Bounds<Pixels>> {
        let pos = self.last_click_position.lock().ok()?.as_ref().copied()?;
        let (x, y) = pos;
        let half = TRAY_ICON_SIZE / 2;
        Some(Bounds::new(
            point(px((x - half) as f32), px((y - half) as f32)),
            size(px(TRAY_ICON_SIZE as f32), px(TRAY_ICON_SIZE as f32)),
        ))
    }

    pub fn shutdown(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.shutdown().wait();
        }
    }
}

impl Drop for LinuxTray {
    fn drop(&mut self) {
        self.shutdown();
    }
}

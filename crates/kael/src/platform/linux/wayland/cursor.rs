use crate::Globals;
use crate::platform::linux::{DEFAULT_CURSOR_ICON_NAME, log_cursor_icon_warning};
use anyhow::{Context as _, anyhow};
use util::ResultExt;

use wayland_client::Connection;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::protocol::{wl_pointer::WlPointer, wl_shm::WlShm};
use wayland_cursor::{CursorImageBuffer, CursorTheme};

pub(crate) struct Cursor {
    loaded_theme: Option<LoadedTheme>,
    size: u32,
    scaled_size: u32,
    surface: WlSurface,
    shm: WlShm,
    connection: Connection,
}

pub(crate) struct LoadedTheme {
    theme: CursorTheme,
    name: Option<String>,
    scaled_size: u32,
}

impl Drop for Cursor {
    fn drop(&mut self) {
        self.loaded_theme.take();
        self.surface.destroy();
    }
}

impl Cursor {
    pub fn new(connection: &Connection, globals: &Globals, size: u32) -> Self {
        let size = size.clamp(1, 256);
        let mut this = Self {
            loaded_theme: None,
            size,
            scaled_size: size,
            surface: globals.compositor.create_surface(&globals.qh, ()),
            shm: globals.shm.clone(),
            connection: connection.clone(),
        };
        this.set_theme_internal(None);
        this
    }

    fn set_theme_internal(&mut self, theme_name: Option<String>) {
        if let Some(loaded_theme) = self.loaded_theme.as_ref()
            && loaded_theme.name == theme_name
            && loaded_theme.scaled_size == self.scaled_size
        {
            return;
        }
        let result = if let Some(theme_name) = theme_name.as_ref() {
            CursorTheme::load_from_name(
                &self.connection,
                self.shm.clone(),
                theme_name,
                self.scaled_size,
            )
        } else {
            CursorTheme::load(&self.connection, self.shm.clone(), self.scaled_size)
        };
        if let Some(theme) = result
            .context("Wayland: Failed to load cursor theme")
            .log_err()
        {
            self.loaded_theme = Some(LoadedTheme {
                theme,
                name: theme_name,
                scaled_size: self.scaled_size,
            });
        }
    }

    pub fn set_theme(&mut self, theme_name: String) {
        self.set_theme_internal(Some(theme_name));
    }

    fn set_scaled_size(&mut self, scaled_size: u32) {
        self.scaled_size = scaled_size.clamp(1, 1_024);
        let theme_name = self
            .loaded_theme
            .as_ref()
            .and_then(|loaded_theme| loaded_theme.name.clone());
        self.set_theme_internal(theme_name);
    }

    pub fn set_size(&mut self, size: u32) {
        self.size = size.clamp(1, 256);
        self.set_scaled_size(self.size);
    }

    pub fn set_icon(
        &mut self,
        wl_pointer: &WlPointer,
        serial_id: u32,
        mut cursor_icon_names: &[&str],
        scale: i32,
    ) {
        if !(1..=16).contains(&scale) {
            return;
        }
        let Some(scaled_size) = self.size.checked_mul(scale as u32) else {
            return;
        };
        self.set_scaled_size(scaled_size);
        cursor_icon_names = &cursor_icon_names[..cursor_icon_names.len().min(32)];

        let Some(loaded_theme) = &mut self.loaded_theme else {
            log::warn!("Wayland: Unable to load cursor themes");
            return;
        };
        let mut theme = &mut loaded_theme.theme;

        let mut buffer: &CursorImageBuffer;
        'outer: {
            for cursor_icon_name in cursor_icon_names {
                if cursor_icon_name.len() > 128 {
                    continue;
                }
                if let Some(cursor) = theme.get_cursor(cursor_icon_name)
                    && cursor.image_count() > 0
                {
                    buffer = &cursor[0];
                    break 'outer;
                }
            }

            if let Some(cursor) = theme.get_cursor(DEFAULT_CURSOR_ICON_NAME)
                && cursor.image_count() > 0
            {
                buffer = &cursor[0];
                log_cursor_icon_warning(anyhow!("wayland: using the default cursor icon"));
            } else {
                log_cursor_icon_warning(anyhow!("wayland: default cursor icon is unavailable"));
                return;
            }
        }

        let (width, height) = buffer.dimensions();
        let (hot_x, hot_y) = buffer.hotspot();
        let (Ok(width), Ok(height), Ok(hot_x), Ok(hot_y)) = (
            i32::try_from(width),
            i32::try_from(height),
            i32::try_from(hot_x),
            i32::try_from(hot_y),
        ) else {
            return;
        };

        self.surface.set_buffer_scale(scale);

        wl_pointer.set_cursor(serial_id, Some(&self.surface), hot_x / scale, hot_y / scale);

        self.surface.attach(Some(buffer), 0, 0);
        self.surface.damage(0, 0, width, height);
        self.surface.commit();
    }
}

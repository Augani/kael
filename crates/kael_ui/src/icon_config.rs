//! Icon configuration for customizing icon asset paths.
//!
//! Kael UI uses its compact bundled icon set by default. Applications can set
//! a custom base path to replace those icons with branded assets.

use once_cell::sync::OnceCell;
use std::sync::RwLock;

static ICON_BASE_PATH: OnceCell<RwLock<String>> = OnceCell::new();

/// Virtual base path for the compact icon set bundled with Kael UI.
pub const BUNDLED_ICON_BASE_PATH: &str = "kael-icons";

/// Sets the base path for icon assets.
///
/// This should be called once at application startup, before any icons are loaded.
/// The path will be used as a prefix when loading named icons.
///
/// # Example
///
/// ```rust
/// use kael_ui::set_icon_base_path;
///
/// // Set icons to be loaded from your application's assets directory
/// set_icon_base_path("assets/icons");
///
/// // Icons are now loaded from assets/icons/{icon-name}.svg instead of the
/// // compact set bundled with Kael UI.
/// ```
///
/// # Arguments
///
/// * `path` - The base path where icon SVG files are located (without trailing slash)
pub fn set_icon_base_path(path: impl Into<String>) {
    let path_string = path.into();
    ICON_BASE_PATH
        .get_or_init(|| RwLock::new(String::new()))
        .write()
        .unwrap()
        .clone_from(&path_string);
}

/// Gets the current icon base path.
///
/// Returns the configured icon base path, or Kael's virtual bundled-icon path.
///
/// # Returns
///
/// The base path for loading icon assets.
pub(crate) fn get_icon_base_path() -> String {
    ICON_BASE_PATH
        .get_or_init(|| RwLock::new(BUNDLED_ICON_BASE_PATH.to_string()))
        .read()
        .unwrap()
        .clone()
}

/// Resolves a named icon to its full path.
///
/// This function combines the configured base path with the icon name.
///
/// # Arguments
///
/// * `name` - The icon name (e.g., "arrow-up", "search")
///
/// # Returns
///
/// The full path to the icon SVG file.
///
/// # Example
///
/// ```rust
/// use kael_ui::icon_config::resolve_icon_path;
///
/// let path = resolve_icon_path("arrow-up");
/// assert_eq!(path, "kael-icons/arrow-up.svg");
/// ```
pub fn resolve_icon_path(name: &str) -> String {
    format!("{}/{}.svg", get_icon_base_path(), name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_default_can_be_replaced_with_a_brand_path() {
        assert_eq!(resolve_icon_path("search"), "kael-icons/search.svg");
        set_icon_base_path("custom/path/icons");
        let path = resolve_icon_path("custom-icon");
        assert_eq!(path, "custom/path/icons/custom-icon.svg");
    }
}

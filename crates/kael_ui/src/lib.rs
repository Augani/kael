#![doc = include_str!("../README.md")]
#![allow(missing_docs)]
#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]

extern crate kael;

pub mod animate;
pub mod animations;
pub mod astryx;
pub mod charts;
pub mod components;
pub mod content_transition;
pub mod devtools;
pub mod display;
pub mod headless;
pub mod kael_ext;
pub mod layout;
pub mod navigation;
pub mod overlays;
pub mod prelude;
pub mod responsive;
pub mod scroll_physics;
pub mod spring;
pub mod styled_ext;
pub mod theme;
pub mod transitions;
pub mod virtual_list;

/// Extension traits for common types
pub mod util;

/// Font loading and registration
pub mod fonts;

/// Icon configuration for custom asset paths
pub mod icon_config;

/// HTTP client for remote image loading
pub mod http;

/// Async data-loading state and query helpers
pub mod query;

// Re-export commonly used icon configuration functions
pub use icon_config::set_icon_base_path;

// Re-export HTTP client functions
pub use http::{init_http, init_http_with_user_agent};

/// Initialize the UI library
///
/// This registers all necessary keybindings and initializes component systems.
/// Registers custom fonts for the component library.
/// Also initializes HTTP client for remote image loading.
pub fn init(cx: &mut kael::App) {
    fonts::register_fonts(cx);
    http::init_http(cx);

    components::input::init(cx);
    components::otp_input::init(cx);
    components::select::init_select(cx);
    components::combobox::init_combobox(cx);
    components::date_picker::init(cx);
    components::drawer_navigation::init_drawer_navigation(cx);
    components::editor::init(cx);
    components::image_viewer::init_image_viewer(cx);
    components::inline_edit::init(cx);
    components::mention_input::init_mention_input(cx);
    components::text_field::init(cx);
    #[cfg(feature = "media")]
    components::video_player::init_video_player(cx);
    navigation::sidebar::init_sidebar(cx);
    navigation::tabs::init_tabs(cx);
    overlays::popover::init(cx);
    overlays::dialog::init_dialog(cx);
    overlays::sheet::init_sheet(cx);
    overlays::alert_dialog::init_alert_dialog(cx);
    overlays::command_palette::init_command_palette(cx);
}

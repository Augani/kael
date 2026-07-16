#![allow(missing_docs)]
#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]

//! # kael_ui: Professional UI Component Library for Kael
//!
//! A comprehensive, themeable component library inspired by shadcn/ui, designed specifically
//! for building polished desktop applications using Kael. Provides a complete set of
//! reusable components with consistent styling, smooth animations, and accessibility support.
//! ## Architecture Overview
//!
//! The library is organized into several key modules:
//! - `theme`: Design tokens and theming system with light/dark variants
//! - `components`: Core interactive elements (buttons, inputs, selects, etc.)
//! - `display`: Presentation components (tables, cards, badges, etc.)
//! - `navigation`: Navigation components (sidebars, menus, tabs, etc.)
//! - `overlays`: Modal dialogs, popovers, tooltips, and command palettes
//! - `animations`: Professional animation presets and easing functions
//!
//! ## Key Features
//!
//! - **Theme System**: Comprehensive design tokens with automatic light/dark mode support
//! - **Accessibility**: Full keyboard navigation, ARIA labels, and screen reader support
//! - **Performance**: Optimized rendering with virtual scrolling for large datasets
//! - **Animation**: Smooth, professional animations using spring physics and easing curves
//! - **Type Safety**: Strong typing throughout with compile-time guarantees
//!
//! ## Design Philosophy
//!
//! Components follow shadcn/ui principles with Kael-specific optimizations:
//! - Composition over inheritance for flexible component APIs
//! - Builder pattern for ergonomic component construction
//! - Entity-based state management for complex interactive components
//! - Consistent naming and styling patterns across all components
//!
//! ## Usage Example
//!
//! ```rust,ignore
//! use kael_ui::{prelude::*, theme};
//!
//! // Initialize theme and components
//! fn init_app(cx: &mut kael::App) {
//!     theme::install_theme(cx, theme::Theme::dark());
//!     kael_ui::init(cx);
//! }
//!
//! // Use components in your views
//! fn render(cx: &mut kael::App) -> impl kael::IntoElement {
//!     div()
//!         .child(Button::new("Click me").on_click(|_, _, _| println!("Clicked!")))
//!         .child(Input::new(&input_state).placeholder("Enter text..."))
//! }
//! ```
//!

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

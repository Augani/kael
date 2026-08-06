#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![allow(clippy::type_complexity)]
#![allow(clippy::collapsible_else_if)]
#![allow(clippy::redundant_clone)]
#![allow(clippy::unnecessary_cast)]
#![allow(clippy::mut_from_ref)]
#![allow(unused_mut)]

extern crate self as kael;

#[macro_use]
mod action;
pub mod accessibility;
/// Explicit animation primitives and keyframe builders.
pub mod animation;
mod app;
pub mod app_runtime;
/// Background job orchestration with worker-pool integration.
pub mod background_jobs;
/// Gesture recognizers and higher-level pointer interaction types.
pub mod gesture;
/// Pre-built panel implementations for common dock areas.
pub mod panels;
/// Workspace and panel layout management with JSON persistence.
pub mod workspace;

mod arena;
mod asset_cache;
mod assets;
#[cfg(feature = "auto-update")]
mod auto_updater;
pub mod benchmark;
mod bounds_tree;
mod cache;
mod clip_path;
mod color;
/// The default colors used by GPUI.
pub mod colors;
/// Command registry for registering named commands invokable from menus,
/// keybindings, and a command palette.
pub mod command_registry;
/// Memoized values with automatic entity-dependency tracking.
pub mod computed;
mod crash_reporter;
/// Developer tools for observability, diagnostics, and runtime inspection.
pub mod dev_tools;
mod element;
mod elements;
mod executor;
pub mod extension_host;
pub mod extension_rpc;
mod file_watcher;
mod geometry;
mod global;
/// GPU memory budgeting and eviction.
/// Golden-image pixel-diff comparison for the headless render pipeline.
pub mod golden;
pub mod gpu;
/// Capability reporting for native graphics and visual escape hatches.
pub mod graphics_capabilities;
/// Headless off-screen rendering for benchmarks and golden-image tests.
pub mod headless_render;
mod icons;
mod input;
mod inspector;
mod interactive;
/// The canonical interpolation vocabulary shared across the framework.
pub mod interpolate;
pub mod ipc_transport;
mod key_dispatch;
mod keymap;
#[cfg(feature = "lottie")]
mod lottie;
pub mod media_capture;
#[cfg(feature = "media")]
pub mod media_playback;
mod path_builder;
mod pixel_snap;
mod platform;
/// Platform capability detection and feature-level support reporting.
pub mod platform_caps;
pub mod plugin;
pub mod prelude;
mod print;
pub mod process_model;
/// Runtime worker support.
pub mod runtime;
mod scene;
/// Scene graph primitives for canvas and creative applications.
pub mod scene_graph;
pub mod scroll_elasticity;
pub mod security;
mod session_store;
#[allow(dead_code)]
mod shadow_cache;
mod shared_string;
mod shared_uri;
/// Split-pane and tab model for IDE-style workspace layouts.
pub mod split_pane;
/// Status bar for displaying contextual information in large applications.
pub mod status_bar;
mod style;
mod styled;
mod subscription;
pub mod supervisor;
mod svg_renderer;
mod tab_stop;
mod taffy;
#[cfg(any(test, feature = "test-support"))]
pub mod test;
/// Text and document editing engine for IDEs, notes apps, and chat composers.
pub mod text_engine;
mod text_system;
/// Application themes with JSON or TOML loading and file hot-reload support.
pub mod theme;
mod tracer;
mod util;
/// Video color: YCbCr→RGB matrices and transfer functions.
pub mod video_color;
mod view;
/// Virtualized data models for lists, tables, and trees.
pub mod virtual_data;
mod webview;
mod window;
/// Worker API for runtime tasks.
pub mod worker_api;

#[cfg(doc)]
pub mod _ownership_and_data_flow;

/// Do not touch, here be dragons for use by kael_macros and such.
#[doc(hidden)]
pub mod private {
    pub use anyhow;
    pub use inventory;
    pub use schemars;
    pub use serde;
    pub use serde_json;
}

mod seal {
    /// A mechanism for restricting implementations of a trait to only those in GPUI.
    /// See: <https://predr.ag/blog/definitive-guide-to-sealed-traits-in-rust/>
    pub trait Sealed {}
}

pub use accessibility::*;
pub use action::*;
pub use anyhow::Result;
pub use app::*;
pub use app_runtime::*;
pub(crate) use arena::*;
pub use asset_cache::*;
pub use assets::*;
#[cfg(feature = "auto-update")]
pub use auto_updater::*;
pub use background_jobs::*;
pub use benchmark::*;
pub use clip_path::*;
pub use color::*;
pub use command_registry::{
    CommandDescriptor, CommandIpcHandoff, CommandIpcHandoffBuilder, CommandIpcNextAction,
    CommandIpcRequest, CommandPalette, PaletteCommandId,
};
pub use computed::*;
pub use crash_reporter::*;
pub use ctor::ctor;
pub use dev_tools::*;
pub use element::*;
pub use elements::*;
pub use executor::*;
pub use extension_host::*;
pub use extension_rpc::*;
pub use file_watcher::*;
pub use geometry::*;
pub use gesture::*;
pub use global::*;
pub use gpu::*;
pub use graphics_capabilities::*;
pub use headless_render::*;
pub use http_client;
pub use input::*;
pub use inspector::*;
pub use interactive::*;
pub use ipc_transport::*;
pub use kael_macros::{AppContext, IntoElement, Render, VisualContext, register_action, test};
#[cfg(feature = "pdf")]
pub use kael_pdf as pdf;
#[cfg(feature = "share")]
pub use kael_share::{
    PlatformShareSupport, ShareFileType, ShareImage, ShareItem, ShareReceiver, ShareResult,
    ShareSheet, ShareSheetBuilder, ShareType, cleanup_share_temps,
};
use key_dispatch::*;
pub use keymap::*;
#[cfg(feature = "lottie")]
pub use lottie::*;
pub use media_capture::*;
#[cfg(feature = "media")]
pub use media_playback::*;
pub use panels::*;
pub use path_builder::*;
pub use pixel_snap::PixelSnapPolicy;
pub use platform::*;
pub use platform_caps::*;
pub use plugin::*;
pub use print::*;
pub use process_model::*;
pub use refineable::*;
pub use runtime::*;
pub use scene::*;
pub use scene_graph::*;
pub use security::*;
pub use session_store::*;
pub use shared_string::*;
pub use shared_uri::*;
pub use smol::Timer;
pub use split_pane::*;
pub use status_bar::*;
pub use style::*;
pub use styled::*;
pub use subscription::*;
pub use supervisor::*;
use svg_renderer::*;
pub(crate) use tab_stop::*;
pub use taffy::{AvailableSpace, LayoutId};
#[cfg(any(test, feature = "test-support"))]
pub use test::*;
pub use text_engine::*;
pub use text_system::*;
pub use theme::*;
pub use tracer::*;
#[cfg(any(test, feature = "test-support"))]
pub use util::smol_timeout;
pub use util::{FutureExt, Timeout, arc_cow::ArcCow};
pub use video_color::*;
pub use view::*;
pub use virtual_data::*;
pub use webview::*;
pub use window::*;
pub use worker_api::*;
pub use workspace::*;

use std::{any::Any, borrow::BorrowMut, future::Future};
use taffy::TaffyLayoutEngine;

/// The context trait, allows the different contexts in GPUI to be used
/// interchangeably for certain operations.
pub trait AppContext {
    /// The result type for this context, used for async contexts that
    /// can't hold a direct reference to the application context.
    type Result<T>;

    /// Create a new entity in the app context.
    #[expect(
        clippy::wrong_self_convention,
        reason = "`App::new` is an ubiquitous function for creating entities"
    )]
    fn new<T: 'static>(
        &mut self,
        build_entity: impl FnOnce(&mut Context<T>) -> T,
    ) -> Self::Result<Entity<T>>;

    /// Reserve a slot for a entity to be inserted later.
    /// The returned [Reservation] allows you to obtain the [EntityId] for the future entity.
    fn reserve_entity<T: 'static>(&mut self) -> Self::Result<Reservation<T>>;

    /// Insert a new entity in the app context based on a [Reservation] previously obtained from [`reserve_entity`].
    ///
    /// [`reserve_entity`]: Self::reserve_entity
    fn insert_entity<T: 'static>(
        &mut self,
        reservation: Reservation<T>,
        build_entity: impl FnOnce(&mut Context<T>) -> T,
    ) -> Self::Result<Entity<T>>;

    /// Update a entity in the app context.
    fn update_entity<T, R>(
        &mut self,
        handle: &Entity<T>,
        update: impl FnOnce(&mut T, &mut Context<T>) -> R,
    ) -> Self::Result<R>
    where
        T: 'static;

    /// Update a entity in the app context.
    fn as_mut<'a, T>(&'a mut self, handle: &Entity<T>) -> Self::Result<GpuiBorrow<'a, T>>
    where
        T: 'static;

    /// Read a entity from the app context.
    fn read_entity<T, R>(
        &self,
        handle: &Entity<T>,
        read: impl FnOnce(&T, &App) -> R,
    ) -> Self::Result<R>
    where
        T: 'static;

    /// Update a window for the given handle.
    fn update_window<T, F>(&mut self, window: AnyWindowHandle, f: F) -> Result<T>
    where
        F: FnOnce(AnyView, &mut Window, &mut App) -> T;

    /// Read a window off of the application context.
    fn read_window<T, R>(
        &self,
        window: &WindowHandle<T>,
        read: impl FnOnce(Entity<T>, &App) -> R,
    ) -> Result<R>
    where
        T: 'static;

    /// Spawn a future on a background thread
    fn background_spawn<R>(&self, future: impl Future<Output = R> + Send + 'static) -> Task<R>
    where
        R: Send + 'static;

    /// Read a global from this app context
    fn read_global<G, R>(&self, callback: impl FnOnce(&G, &App) -> R) -> Self::Result<R>
    where
        G: Global;
}

/// Returned by [Context::reserve_entity] to later be passed to [Context::insert_entity].
/// Allows you to obtain the [EntityId] for a entity before it is created.
pub struct Reservation<T>(pub(crate) Slot<T>);

impl<T: 'static> Reservation<T> {
    /// Returns the [EntityId] that will be associated with the entity once it is inserted.
    pub fn entity_id(&self) -> EntityId {
        self.0.entity_id()
    }
}

/// This trait is used for the different visual contexts in GPUI that
/// require a window to be present.
pub trait VisualContext: AppContext {
    /// Returns the handle of the window associated with this context.
    fn window_handle(&self) -> AnyWindowHandle;

    /// Invalidates retained subtree cache state for elements with the given id in this context's window.
    fn invalidate_cache(&mut self, element_id: impl Into<ElementId>) -> Result<()> {
        let window = self.window_handle();
        let element_id = element_id.into();
        self.update_window(window, move |_, window, _| {
            window.invalidate_cache(element_id);
        })
    }

    /// Update a view with the given callback
    fn update_window_entity<T: 'static, R>(
        &mut self,
        entity: &Entity<T>,
        update: impl FnOnce(&mut T, &mut Window, &mut Context<T>) -> R,
    ) -> Self::Result<R>;

    /// Create a new entity, with access to `Window`.
    fn new_window_entity<T: 'static>(
        &mut self,
        build_entity: impl FnOnce(&mut Window, &mut Context<T>) -> T,
    ) -> Self::Result<Entity<T>>;

    /// Replace the root view of a window with a new view.
    fn replace_root_view<V>(
        &mut self,
        build_view: impl FnOnce(&mut Window, &mut Context<V>) -> V,
    ) -> Self::Result<Entity<V>>
    where
        V: 'static + Render;

    /// Focus a entity in the window, if it implements the [`Focusable`] trait.
    fn focus<V>(&mut self, entity: &Entity<V>) -> Self::Result<()>
    where
        V: Focusable;
}

/// A trait for tying together the types of a GPUI entity and the events it can
/// emit.
pub trait EventEmitter<E: Any>: 'static {}

/// A helper trait for auto-implementing certain methods on contexts that
/// can be used interchangeably.
pub trait BorrowAppContext {
    /// Set a global value on the context.
    fn set_global<T: Global>(&mut self, global: T);
    /// Updates the global state of the given type.
    fn update_global<G, R>(&mut self, f: impl FnOnce(&mut G, &mut Self) -> R) -> R
    where
        G: Global;
    /// Updates the global state of the given type, creating a default if it didn't exist before.
    fn update_default_global<G, R>(&mut self, f: impl FnOnce(&mut G, &mut Self) -> R) -> R
    where
        G: Global + Default;
}

impl<C> BorrowAppContext for C
where
    C: BorrowMut<App>,
{
    fn set_global<G: Global>(&mut self, global: G) {
        self.borrow_mut().set_global(global)
    }

    #[track_caller]
    fn update_global<G, R>(&mut self, f: impl FnOnce(&mut G, &mut Self) -> R) -> R
    where
        G: Global,
    {
        let mut global = self.borrow_mut().lease_global::<G>();
        let result = f(&mut global, self);
        self.borrow_mut().end_global_lease(global);
        result
    }

    fn update_default_global<G, R>(&mut self, f: impl FnOnce(&mut G, &mut Self) -> R) -> R
    where
        G: Global + Default,
    {
        self.borrow_mut().default_global::<G>();
        self.update_global(f)
    }
}

/// A flatten equivalent for anyhow `Result`s.
pub trait Flatten<T> {
    /// Convert this type into a simple `Result<T>`.
    fn flatten(self) -> Result<T>;
}

impl<T> Flatten<T> for Result<Result<T>> {
    fn flatten(self) -> Result<T> {
        self?
    }
}

impl<T> Flatten<T> for Result<T> {
    fn flatten(self) -> Result<T> {
        self
    }
}

/// Information about the GPU GPUI is running on.
#[derive(Default, Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct GpuSpecs {
    /// Whether the GPU is really a fake (like `llvmpipe`) running on the CPU.
    pub is_software_emulated: bool,
    /// The name of the device, as reported by Vulkan.
    pub device_name: String,
    /// The name of the driver, as reported by Vulkan.
    pub driver_name: String,
    /// Further information about the driver, as reported by Vulkan.
    pub driver_info: String,
}

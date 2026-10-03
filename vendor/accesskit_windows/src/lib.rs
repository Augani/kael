// Copyright 2021 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file) or the MIT license (found in
// the LICENSE-MIT file), at your option.

#[cfg(target_os = "windows")]
mod context;
#[cfg(target_os = "windows")]
mod filters;
#[cfg(target_os = "windows")]
mod node;
#[cfg(target_os = "windows")]
mod nullable_text_range;
#[cfg(target_os = "windows")]
mod text;
#[cfg(target_os = "windows")]
mod util;
#[cfg(target_os = "windows")]
mod window_handle;

#[cfg(target_os = "windows")]
mod adapter;
#[cfg(target_os = "windows")]
pub use adapter::{Adapter, QueuedEvents};

#[cfg(target_os = "windows")]
mod subclass;
#[cfg(target_os = "windows")]
pub use subclass::SubclassingAdapter;

#[cfg(target_os = "windows")]
pub use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};

#[cfg(all(test, target_os = "windows"))]
mod tests;

#[cfg(any(test, target_os = "windows"))]
mod text_queries;

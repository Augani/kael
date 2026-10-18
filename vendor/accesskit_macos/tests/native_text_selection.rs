// Licensed under the Apache License, Version 2.0 or MIT, matching the adapter.
//! Actual NSAccessibility UTF-16 getters/setters on the process main thread.

#[cfg(target_os = "macos")]
#[path = "native_outline/macos.rs"]
mod native;
#[cfg(target_os = "macos")]
use native::{context, filters, node, text_edit, util};

#[cfg(target_os = "macos")]
fn main() {
    native::run_text_selection();
}
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!(
        "SKIP native_text_selection: AppKit verification requires macOS; macOS CI must execute it."
    );
}

// Licensed under the Apache License, Version 2.0 or MIT, matching the adapter.
//! Native NSAccessibility protocol integration, with no visible window.
//! A harness-free test keeps all AppKit objects on the process's main thread.

#[cfg(target_os = "macos")]
#[path = "native_outline/macos.rs"]
mod native;
#[cfg(target_os = "macos")]
use native::{context, filters, node, text_edit, util};

#[cfg(target_os = "macos")]
fn main() {
    native::run();
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!(
        "SKIP native_outline: AppKit protocol verification requires macOS; the macOS CI job must execute it."
    );
}

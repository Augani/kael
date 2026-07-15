use std::ops::Deref;

use windows::Win32::{Foundation::HWND, UI::WindowsAndMessaging::HCURSOR};

#[derive(Debug, Clone, Copy)]
pub(crate) struct SafeCursor {
    raw: HCURSOR,
}

// SAFETY: `HCURSOR` is a copyable system handle. This wrapper does not own or destroy
// it; the cached cursors are `LR_SHARED` resources and are only passed back to Win32.
unsafe impl Send for SafeCursor {}
// SAFETY: See the `Send` rationale; immutable handle copies carry no Rust aliasing state.
unsafe impl Sync for SafeCursor {}

impl From<HCURSOR> for SafeCursor {
    fn from(value: HCURSOR) -> Self {
        SafeCursor { raw: value }
    }
}

impl Deref for SafeCursor {
    type Target = HCURSOR;

    fn deref(&self) -> &Self::Target {
        &self.raw
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SafeHwnd {
    raw: HWND,
}

impl SafeHwnd {
    pub(crate) fn as_raw(&self) -> HWND {
        self.raw
    }
}

// SAFETY: `HWND` is an opaque copyable identifier. Cross-thread users in this module
// only retain it for Win32's thread-safe message-posting/enumeration APIs; window state
// remains owned by its creating UI thread.
unsafe impl Send for SafeHwnd {}
// SAFETY: Sharing the identifier does not share Rust-managed window memory.
unsafe impl Sync for SafeHwnd {}

impl From<HWND> for SafeHwnd {
    fn from(value: HWND) -> Self {
        SafeHwnd { raw: value }
    }
}

impl Deref for SafeHwnd {
    type Target = HWND;

    fn deref(&self) -> &Self::Target {
        &self.raw
    }
}

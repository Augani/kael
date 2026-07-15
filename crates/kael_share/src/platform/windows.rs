use anyhow::{Result, anyhow};

use crate::{
    ReceiverCallback, ShareFileType, ShareResult, ShareSheet, ShareType,
    platform::PlatformShareReceiver,
};

pub(crate) async fn show(sheet: &ShareSheet) -> Result<ShareResult> {
    if sheet.file_attachment_count() > 0 || sheet.image_count() > 0 {
        return Err(anyhow!(
            "Windows file and image sharing still needs DataTransferManager integration"
        ));
    }

    if !sheet.is_excluded(ShareType::Mail) {
        if let Some(uri) = sheet.mailto_uri() {
            if open_with_shell(&uri)? {
                return Ok(ShareResult::Completed {
                    activity_type: ShareType::Mail.activity_name().to_string(),
                });
            }
        }
    }

    if !sheet.is_excluded(ShareType::Clipboard) {
        if let Some(body) = sheet.body_text() {
            if copy_to_clipboard(&body)? {
                return Ok(ShareResult::Completed {
                    activity_type: ShareType::Clipboard.activity_name().to_string(),
                });
            }
        }
    }

    Ok(ShareResult::Cancelled)
}

pub(crate) fn register_receiver(
    _file_types: &[ShareFileType],
    _callback: ReceiverCallback,
) -> Result<PlatformShareReceiver> {
    Err(anyhow!(
        "share receiver registration is not implemented yet on Windows"
    ))
}

pub(crate) fn support() -> crate::PlatformShareSupport {
    crate::PlatformShareSupport {
        mail: true,
        messages: false,
        airdrop: false,
        clipboard: true,
        social: false,
        print: false,
        receiver_registration: false,
    }
}

fn copy_to_clipboard(text: &str) -> Result<bool> {
    use windows::Win32::{
        Foundation::{GlobalFree, HANDLE, HGLOBAL},
        System::{
            DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
        },
    };

    const CF_UNICODETEXT: u32 = 13;
    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();

    struct ClipboardGuard;
    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }

    struct GlobalMemoryGuard(HGLOBAL);
    impl Drop for GlobalMemoryGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = GlobalFree(Some(self.0));
            }
        }
    }

    unsafe {
        OpenClipboard(None)?;
        let _clipboard = ClipboardGuard;

        EmptyClipboard()?;

        let allocation = match GlobalAlloc(GMEM_MOVEABLE, std::mem::size_of_val(wide.as_slice())) {
            Ok(alloc) => alloc,
            Err(_) => return Ok(false),
        };
        let allocation_guard = GlobalMemoryGuard(allocation);

        let target = GlobalLock(allocation) as *mut u16;
        if target.is_null() {
            return Ok(false);
        }

        target.copy_from_nonoverlapping(wide.as_ptr(), wide.len());
        let _ = GlobalUnlock(allocation);
        SetClipboardData(CF_UNICODETEXT, Some(HANDLE(allocation.0)))?;
        std::mem::forget(allocation_guard);
    }

    Ok(true)
}

fn open_with_shell(target: &str) -> Result<bool> {
    use windows::{
        Win32::{UI::Shell::ShellExecuteW, UI::WindowsAndMessaging::SW_SHOWNORMAL},
        core::PCWSTR,
    };

    let operation: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
    let target: Vec<u16> = target.encode_utf16().chain(Some(0)).collect();

    unsafe {
        let instance = ShellExecuteW(
            None,
            PCWSTR::from_raw(operation.as_ptr()),
            PCWSTR::from_raw(target.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
        Ok(instance.0 as usize > 32)
    }
}

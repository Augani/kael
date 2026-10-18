// Copyright 2026 The Kael contributors. All rights reserved.
// Licensed under Apache-2.0 OR MIT. See PATCHES.md for upstream provenance.
//! Identical UIA ABI with an explicitly nullable FindText success result.
//! windows-rs's generated Result<ITextRangeProvider> cannot express S_OK + NULL.
use windows::{Win32::UI::Accessibility::*, core::*};
windows_core::imp::define_interface!(
    NullableTextRange,
    NullableTextRange_Vtbl,
    0x5347ad7b_c355_46f8_aff5_909033582f63
);
windows_core::imp::interface_hierarchy!(NullableTextRange, IUnknown);
#[repr(C)]
pub struct NullableTextRange_Vtbl(pub ITextRangeProvider_Vtbl);
#[allow(non_camel_case_types)]
pub trait NullableTextRange_Impl: ITextRangeProvider_Impl {
    fn find_text_nullable(
        &self,
        text: &BSTR,
        backward: BOOL,
        ignore_case: BOOL,
    ) -> Result<Option<ITextRangeProvider>>;
}
impl NullableTextRange_Vtbl {
    pub const fn new<Identity: NullableTextRange_Impl, const OFFSET: isize>() -> Self {
        unsafe extern "system" fn find_text<
            Identity: NullableTextRange_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            text: *mut core::ffi::c_void,
            backward: BOOL,
            ignore_case: BOOL,
            result: *mut *mut core::ffi::c_void,
        ) -> HRESULT {
            if result.is_null() {
                return HRESULT(0x80004003u32 as i32);
            }
            // All success/error paths initialize the caller's output. Never
            // construct a Rust interface around a null COM pointer.
            unsafe {
                result.write(core::ptr::null_mut());
                let identity = &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                match identity.find_text_nullable(
                    core::mem::transmute::<&*mut core::ffi::c_void, &BSTR>(&text),
                    backward,
                    ignore_case,
                ) {
                    Ok(Some(range)) => {
                        result.write(range.into_raw());
                        HRESULT(0)
                    }
                    Ok(None) => HRESULT(0),
                    Err(error) => error.into(),
                }
            }
        }
        let mut vtable = ITextRangeProvider_Vtbl::new::<Identity, OFFSET>();
        vtable.FindText = find_text::<Identity, OFFSET>;
        Self(vtable)
    }
    pub fn matches(iid: &GUID) -> bool {
        iid == &ITextRangeProvider::IID
    }
}
impl RuntimeName for NullableTextRange {}

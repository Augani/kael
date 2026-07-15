#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

mod bindings;

#[cfg(target_os = "macos")]
pub mod core_media {
    #![allow(non_snake_case)]

    pub use crate::bindings::{
        CMItemIndex, CMSampleTimingInfo, CMTime, CMTimeMake, CMVideoCodecType,
        kCMSampleAttachmentKey_NotSync, kCMTimeInvalid, kCMVideoCodecType_H264,
    };
    use anyhow::Result;
    use core_foundation::{
        array::{CFArray, CFArrayRef},
        base::{CFTypeID, OSStatus, TCFType},
        declare_TCFType,
        dictionary::CFDictionary,
        impl_CFTypeDescription, impl_TCFType,
        string::CFString,
    };
    use core_video::image_buffer::{CVImageBuffer, CVImageBufferRef};
    use std::{ffi::c_void, ptr};

    #[repr(C)]
    pub struct __CMSampleBuffer(c_void);
    // The ref type must be a pointer to the underlying struct.
    pub type CMSampleBufferRef = *const __CMSampleBuffer;

    declare_TCFType!(CMSampleBuffer, CMSampleBufferRef);
    impl_TCFType!(CMSampleBuffer, CMSampleBufferRef, CMSampleBufferGetTypeID);
    impl_CFTypeDescription!(CMSampleBuffer);

    impl CMSampleBuffer {
        pub fn attachments(&self) -> Vec<CFDictionary<CFString>> {
            unsafe {
                let attachments =
                    CMSampleBufferGetSampleAttachmentsArray(self.as_concrete_TypeRef(), true);
                if attachments.is_null() {
                    return Vec::new();
                }
                CFArray::<CFDictionary>::wrap_under_get_rule(attachments)
                    .into_iter()
                    .map(|attachments| {
                        CFDictionary::wrap_under_get_rule(attachments.as_concrete_TypeRef())
                    })
                    .collect()
            }
        }

        pub fn image_buffer(&self) -> Option<CVImageBuffer> {
            unsafe {
                let ptr = CMSampleBufferGetImageBuffer(self.as_concrete_TypeRef());
                if ptr.is_null() {
                    None
                } else {
                    Some(CVImageBuffer::wrap_under_get_rule(ptr))
                }
            }
        }

        pub fn sample_timing_info(&self, index: usize) -> Result<CMSampleTimingInfo> {
            unsafe {
                let index = checked_item_index(index)?;
                let mut timing_info = CMSampleTimingInfo {
                    duration: kCMTimeInvalid,
                    presentationTimeStamp: kCMTimeInvalid,
                    decodeTimeStamp: kCMTimeInvalid,
                };
                let result = CMSampleBufferGetSampleTimingInfo(
                    self.as_concrete_TypeRef(),
                    index,
                    &mut timing_info,
                );
                anyhow::ensure!(
                    result == 0,
                    "error getting sample timing info, code {result}"
                );
                Ok(timing_info)
            }
        }

        pub fn format_description(&self) -> Option<CMFormatDescription> {
            unsafe {
                let description = CMSampleBufferGetFormatDescription(self.as_concrete_TypeRef());
                if description.is_null() {
                    None
                } else {
                    Some(CMFormatDescription::wrap_under_get_rule(description))
                }
            }
        }

        pub fn data(&self) -> Option<CMBlockBuffer> {
            unsafe {
                let ptr = CMSampleBufferGetDataBuffer(self.as_concrete_TypeRef());
                if ptr.is_null() {
                    None
                } else {
                    Some(CMBlockBuffer::wrap_under_get_rule(ptr))
                }
            }
        }
    }

    fn checked_item_index(index: usize) -> Result<CMItemIndex> {
        CMItemIndex::try_from(index)
            .map_err(|_| anyhow::anyhow!("sample timing index {index} is out of range"))
    }

    #[cfg(test)]
    mod tests {
        use super::checked_item_index;

        #[test]
        fn sample_timing_index_must_fit_core_media_signed_index() {
            assert_eq!(checked_item_index(0).unwrap(), 0);
            assert!(checked_item_index(usize::MAX).is_err());
        }
    }

    #[link(name = "CoreMedia", kind = "framework")]
    unsafe extern "C" {
        fn CMSampleBufferGetTypeID() -> CFTypeID;
        fn CMSampleBufferGetSampleAttachmentsArray(
            buffer: CMSampleBufferRef,
            create_if_necessary: bool,
        ) -> CFArrayRef;
        fn CMSampleBufferGetImageBuffer(buffer: CMSampleBufferRef) -> CVImageBufferRef;
        fn CMSampleBufferGetSampleTimingInfo(
            buffer: CMSampleBufferRef,
            index: CMItemIndex,
            timing_info_out: *mut CMSampleTimingInfo,
        ) -> OSStatus;
        fn CMSampleBufferGetFormatDescription(buffer: CMSampleBufferRef) -> CMFormatDescriptionRef;
        fn CMSampleBufferGetDataBuffer(sample_buffer: CMSampleBufferRef) -> CMBlockBufferRef;
    }

    #[repr(C)]
    pub struct __CMFormatDescription(c_void);
    pub type CMFormatDescriptionRef = *const __CMFormatDescription;

    declare_TCFType!(CMFormatDescription, CMFormatDescriptionRef);
    impl_TCFType!(
        CMFormatDescription,
        CMFormatDescriptionRef,
        CMFormatDescriptionGetTypeID
    );
    impl_CFTypeDescription!(CMFormatDescription);

    impl CMFormatDescription {
        pub fn h264_parameter_set_count(&self) -> Result<usize> {
            unsafe {
                let mut count = 0;
                let result = CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
                    self.as_concrete_TypeRef(),
                    0,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    &mut count,
                    ptr::null_mut(),
                );
                anyhow::ensure!(
                    result == 0,
                    "error getting parameter set count, code: {result}"
                );
                Ok(count)
            }
        }

        pub fn h264_parameter_set_at_index(&self, index: usize) -> Result<&[u8]> {
            unsafe {
                let mut bytes = ptr::null();
                let mut len = 0;
                let result = CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
                    self.as_concrete_TypeRef(),
                    index,
                    &mut bytes,
                    &mut len,
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
                anyhow::ensure!(result == 0, "error getting parameter set, code: {result}");
                if len == 0 {
                    return Ok(&[]);
                }
                anyhow::ensure!(
                    !bytes.is_null(),
                    "parameter set returned a null data pointer"
                );
                Ok(std::slice::from_raw_parts(bytes, len))
            }
        }
    }

    #[link(name = "CoreMedia", kind = "framework")]
    unsafe extern "C" {
        fn CMFormatDescriptionGetTypeID() -> CFTypeID;
        fn CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
            video_desc: CMFormatDescriptionRef,
            parameter_set_index: usize,
            parameter_set_pointer_out: *mut *const u8,
            parameter_set_size_out: *mut usize,
            parameter_set_count_out: *mut usize,
            NALUnitHeaderLengthOut: *mut isize,
        ) -> OSStatus;
    }

    #[repr(C)]
    pub struct __CMBlockBuffer(c_void);
    pub type CMBlockBufferRef = *const __CMBlockBuffer;

    declare_TCFType!(CMBlockBuffer, CMBlockBufferRef);
    impl_TCFType!(CMBlockBuffer, CMBlockBufferRef, CMBlockBufferGetTypeID);
    impl_CFTypeDescription!(CMBlockBuffer);

    impl CMBlockBuffer {
        pub fn bytes(&self) -> Result<&[u8]> {
            unsafe {
                let mut bytes = ptr::null();
                let mut contiguous_len = 0;
                let mut total_len = 0;
                let result = CMBlockBufferGetDataPointer(
                    self.as_concrete_TypeRef(),
                    0,
                    &mut contiguous_len,
                    &mut total_len,
                    &mut bytes,
                );
                anyhow::ensure!(
                    result == 0,
                    "could not get block buffer data, code: {result}"
                );
                if total_len == 0 {
                    return Ok(&[]);
                }
                anyhow::ensure!(!bytes.is_null(), "block buffer returned null data pointer");
                anyhow::ensure!(
                    contiguous_len == total_len,
                    "block buffer is non-contiguous; use copy_bytes() to read all data"
                );
                Ok(std::slice::from_raw_parts(bytes, total_len))
            }
        }

        /// Copies all bytes from this block buffer, including non-contiguous buffers.
        pub fn copy_bytes(&self) -> Result<Vec<u8>> {
            unsafe {
                let len = CMBlockBufferGetDataLength(self.as_concrete_TypeRef());
                let mut bytes = Vec::new();
                bytes.try_reserve_exact(len).map_err(|error| {
                    anyhow::anyhow!("could not allocate {len} bytes for block buffer: {error}")
                })?;
                bytes.resize(len, 0);
                if len == 0 {
                    return Ok(bytes);
                }
                let result = CMBlockBufferCopyDataBytes(
                    self.as_concrete_TypeRef(),
                    0,
                    len,
                    bytes.as_mut_ptr().cast(),
                );
                anyhow::ensure!(
                    result == 0,
                    "could not copy block buffer data, code: {result}"
                );
                Ok(bytes)
            }
        }
    }

    #[link(name = "CoreMedia", kind = "framework")]
    unsafe extern "C" {
        fn CMBlockBufferGetTypeID() -> CFTypeID;
        fn CMBlockBufferGetDataPointer(
            buffer: CMBlockBufferRef,
            offset: usize,
            length_at_offset_out: *mut usize,
            total_length_out: *mut usize,
            data_pointer_out: *mut *const u8,
        ) -> OSStatus;
        fn CMBlockBufferGetDataLength(buffer: CMBlockBufferRef) -> usize;
        fn CMBlockBufferCopyDataBytes(
            buffer: CMBlockBufferRef,
            offset_to_data: usize,
            data_length: usize,
            destination: *mut c_void,
        ) -> OSStatus;
    }
}

#[cfg(target_os = "macos")]
pub mod core_video {
    #![allow(non_snake_case)]

    #[cfg(target_os = "macos")]
    use core_foundation::{
        base::{CFTypeID, TCFType},
        declare_TCFType, impl_CFTypeDescription, impl_TCFType,
    };
    #[cfg(target_os = "macos")]
    use std::ffi::c_void;

    use crate::bindings::{CVReturn, kCVReturnSuccess};
    pub use crate::bindings::{
        kCVPixelFormatType_32BGRA, kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
        kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, kCVPixelFormatType_420YpCbCr8Planar,
    };
    use anyhow::Result;
    use core_foundation::{
        base::kCFAllocatorDefault, dictionary::CFDictionaryRef, mach_port::CFAllocatorRef,
    };
    use foreign_types::ForeignTypeRef;

    use metal::{MTLDevice, MTLPixelFormat};
    use std::ptr;

    #[repr(C)]
    pub struct __CVMetalTextureCache(c_void);
    pub type CVMetalTextureCacheRef = *const __CVMetalTextureCache;

    declare_TCFType!(CVMetalTextureCache, CVMetalTextureCacheRef);
    impl_TCFType!(
        CVMetalTextureCache,
        CVMetalTextureCacheRef,
        CVMetalTextureCacheGetTypeID
    );
    impl_CFTypeDescription!(CVMetalTextureCache);

    impl CVMetalTextureCache {
        /// # Safety
        ///
        /// metal_device must be valid according to CVMetalTextureCacheCreate
        pub unsafe fn new(metal_device: *mut MTLDevice) -> Result<Self> {
            let mut this = ptr::null();
            let result = unsafe {
                CVMetalTextureCacheCreate(
                    kCFAllocatorDefault,
                    ptr::null(),
                    metal_device,
                    ptr::null(),
                    &mut this,
                )
            };
            anyhow::ensure!(
                result == kCVReturnSuccess,
                "could not create texture cache, code: {result}"
            );
            anyhow::ensure!(
                !this.is_null(),
                "texture cache creation returned a null object"
            );
            unsafe { Ok(CVMetalTextureCache::wrap_under_create_rule(this)) }
        }

        /// # Safety
        ///
        /// The arguments to this function must be valid according to CVMetalTextureCacheCreateTextureFromImage
        pub unsafe fn create_texture_from_image(
            &self,
            source: ::core_video::image_buffer::CVImageBufferRef,
            texture_attributes: CFDictionaryRef,
            pixel_format: MTLPixelFormat,
            width: usize,
            height: usize,
            plane_index: usize,
        ) -> Result<CVMetalTexture> {
            let mut this = ptr::null();
            let result = unsafe {
                CVMetalTextureCacheCreateTextureFromImage(
                    kCFAllocatorDefault,
                    self.as_concrete_TypeRef(),
                    source,
                    texture_attributes,
                    pixel_format,
                    width,
                    height,
                    plane_index,
                    &mut this,
                )
            };
            anyhow::ensure!(
                result == kCVReturnSuccess,
                "could not create texture, code: {result}"
            );
            anyhow::ensure!(!this.is_null(), "texture creation returned a null object");
            unsafe { Ok(CVMetalTexture::wrap_under_create_rule(this)) }
        }
    }

    #[link(name = "CoreVideo", kind = "framework")]
    unsafe extern "C" {
        fn CVMetalTextureCacheGetTypeID() -> CFTypeID;
        fn CVMetalTextureCacheCreate(
            allocator: CFAllocatorRef,
            cache_attributes: CFDictionaryRef,
            metal_device: *const MTLDevice,
            texture_attributes: CFDictionaryRef,
            cache_out: *mut CVMetalTextureCacheRef,
        ) -> CVReturn;
        fn CVMetalTextureCacheCreateTextureFromImage(
            allocator: CFAllocatorRef,
            texture_cache: CVMetalTextureCacheRef,
            source_image: ::core_video::image_buffer::CVImageBufferRef,
            texture_attributes: CFDictionaryRef,
            pixel_format: MTLPixelFormat,
            width: usize,
            height: usize,
            plane_index: usize,
            texture_out: *mut CVMetalTextureRef,
        ) -> CVReturn;
    }

    #[repr(C)]
    pub struct __CVMetalTexture(c_void);
    pub type CVMetalTextureRef = *const __CVMetalTexture;

    declare_TCFType!(CVMetalTexture, CVMetalTextureRef);
    impl_TCFType!(CVMetalTexture, CVMetalTextureRef, CVMetalTextureGetTypeID);
    impl_CFTypeDescription!(CVMetalTexture);

    impl CVMetalTexture {
        pub fn as_texture_ref(&self) -> Option<&metal::TextureRef> {
            unsafe {
                let texture = CVMetalTextureGetTexture(self.as_concrete_TypeRef());
                if texture.is_null() {
                    None
                } else {
                    Some(metal::TextureRef::from_ptr(texture.cast()))
                }
            }
        }
    }

    #[link(name = "CoreVideo", kind = "framework")]
    unsafe extern "C" {
        fn CVMetalTextureGetTypeID() -> CFTypeID;
        fn CVMetalTextureGetTexture(texture: CVMetalTextureRef) -> *mut c_void;
    }
}

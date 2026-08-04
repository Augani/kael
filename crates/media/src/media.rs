#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

mod bindings;

/// Safe ownership wrappers around the CoreMedia sample-buffer APIs Kael uses.
#[cfg(target_os = "macos")]
pub mod core_media {
    pub use crate::bindings::{
        CMItemIndex, CMSampleTimingInfo, CMTime, CMTimeMake, CMVideoCodecType, kCMTimeInvalid,
        kCMVideoCodecType_H264,
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

    #[doc(hidden)]
    #[repr(C)]
    pub struct __CMSampleBuffer(c_void);
    // The ref type must be a pointer to the underlying struct.
    /// Borrowed CoreMedia sample-buffer pointer.
    pub type CMSampleBufferRef = *const __CMSampleBuffer;

    declare_TCFType! {
        /// Retained CoreMedia sample buffer.
        CMSampleBuffer, CMSampleBufferRef
    }
    impl_TCFType!(CMSampleBuffer, CMSampleBufferRef, CMSampleBufferGetTypeID);
    impl_CFTypeDescription!(CMSampleBuffer);

    impl CMSampleBuffer {
        /// Return the sample attachment dictionaries, creating the array when needed.
        pub fn attachments(&self) -> Vec<CFDictionary<CFString>> {
            // SAFETY: `self` owns a valid sample buffer. CoreMedia returns a
            // borrowed array whose elements remain valid while the retained
            // wrappers are created, and both wrapper calls follow the get rule.
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

        /// Return the image buffer carried by this sample, if present.
        pub fn image_buffer(&self) -> Option<CVImageBuffer> {
            // SAFETY: `self` owns a valid sample buffer. A non-null image-buffer
            // result is borrowed from it and is retained by `wrap_under_get_rule`.
            unsafe {
                let ptr = CMSampleBufferGetImageBuffer(self.as_concrete_TypeRef());
                if ptr.is_null() {
                    None
                } else {
                    Some(CVImageBuffer::wrap_under_get_rule(ptr))
                }
            }
        }

        /// Return timing information for the sample at `index`.
        pub fn sample_timing_info(&self, index: usize) -> Result<CMSampleTimingInfo> {
            // SAFETY: `self` owns a valid sample buffer, `index` is converted to
            // CoreMedia's signed index type, and the out pointer is writable for
            // one fully initialized `CMSampleTimingInfo` value.
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

        /// Return the sample's format description, if present.
        pub fn format_description(&self) -> Option<CMFormatDescription> {
            // SAFETY: `self` owns a valid sample buffer. A non-null description
            // is borrowed from it and retained by `wrap_under_get_rule`.
            unsafe {
                let description = CMSampleBufferGetFormatDescription(self.as_concrete_TypeRef());
                if description.is_null() {
                    None
                } else {
                    Some(CMFormatDescription::wrap_under_get_rule(description))
                }
            }
        }

        /// Return the sample's encoded data buffer, if present.
        pub fn data(&self) -> Option<CMBlockBuffer> {
            // SAFETY: `self` owns a valid sample buffer. A non-null data buffer
            // is borrowed from it and retained by `wrap_under_get_rule`.
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

    /// Return the CoreMedia attachment key that marks a sample as non-sync.
    pub fn sample_attachment_key_not_sync() -> CFString {
        let key = crate::bindings::sample_attachment_key_not_sync_ref();
        // SAFETY: the bindings module guarantees that `key` is CoreMedia's
        // process-lifetime CFString constant; the get rule retains it for the wrapper.
        unsafe { CFString::wrap_under_get_rule(key) }
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

    #[doc(hidden)]
    #[repr(C)]
    pub struct __CMFormatDescription(c_void);
    /// Borrowed CoreMedia format-description pointer.
    pub type CMFormatDescriptionRef = *const __CMFormatDescription;

    declare_TCFType! {
        /// Retained CoreMedia format description.
        CMFormatDescription, CMFormatDescriptionRef
    }
    impl_TCFType!(
        CMFormatDescription,
        CMFormatDescriptionRef,
        CMFormatDescriptionGetTypeID
    );
    impl_CFTypeDescription!(CMFormatDescription);

    impl CMFormatDescription {
        /// Return the number of H.264 parameter sets in this description.
        pub fn h264_parameter_set_count(&self) -> Result<usize> {
            // SAFETY: `self` owns a valid format description. The optional data
            // outputs are null as permitted by CoreMedia and `count` is a valid
            // writable out parameter.
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

        /// Borrow the H.264 parameter set at `index`.
        ///
        /// The returned slice is tied to this format description and must not
        /// outlive it.
        pub fn h264_parameter_set_at_index(&self, index: usize) -> Result<&[u8]> {
            // SAFETY: `self` owns a valid format description and all provided
            // out pointers are writable. CoreMedia owns the returned bytes for
            // the lifetime of the description; null is rejected when len > 0.
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

    #[doc(hidden)]
    #[repr(C)]
    pub struct __CMBlockBuffer(c_void);
    /// Borrowed CoreMedia block-buffer pointer.
    pub type CMBlockBufferRef = *const __CMBlockBuffer;

    declare_TCFType! {
        /// Retained CoreMedia block buffer.
        CMBlockBuffer, CMBlockBufferRef
    }
    impl_TCFType!(CMBlockBuffer, CMBlockBufferRef, CMBlockBufferGetTypeID);
    impl_CFTypeDescription!(CMBlockBuffer);

    impl CMBlockBuffer {
        /// Borrow all bytes when the block buffer is contiguous.
        ///
        /// Use [`Self::copy_bytes`] for non-contiguous buffers.
        pub fn bytes(&self) -> Result<&[u8]> {
            // SAFETY: `self` owns a valid block buffer and all out pointers are
            // writable. The returned pointer is only exposed when non-null and
            // the requested range is contiguous, with a lifetime tied to self.
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
            // SAFETY: `self` owns a valid block buffer. The vector is resized to
            // the exact reported length before CoreMedia writes that many bytes
            // through its non-null allocation.
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

/// Safe ownership wrappers around Kael's CoreVideo-to-Metal texture bridge.
#[cfg(target_os = "macos")]
pub mod core_video {
    use core_foundation::{
        base::{CFTypeID, TCFType},
        declare_TCFType, impl_CFTypeDescription, impl_TCFType,
    };
    use std::ffi::c_void;

    use ::core_video::pixel_buffer::{CVPixelBuffer, CVPixelBufferRef};
    pub use ::core_video::pixel_buffer::{
        kCVPixelFormatType_32BGRA, kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
        kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, kCVPixelFormatType_420YpCbCr8Planar,
    };
    use ::core_video::r#return::{CVReturn, kCVReturnSuccess};
    use anyhow::Result;
    use core_foundation::{
        base::kCFAllocatorDefault,
        dictionary::{CFDictionary, CFDictionaryRef},
        mach_port::CFAllocatorRef,
    };
    use foreign_types::ForeignTypeRef;

    use metal::{MTLDevice, MTLPixelFormat};
    use std::ptr;

    #[doc(hidden)]
    #[repr(C)]
    pub struct __CVMetalTextureCache(c_void);
    /// Borrowed CoreVideo Metal texture-cache pointer.
    pub type CVMetalTextureCacheRef = *const __CVMetalTextureCache;

    declare_TCFType! {
        /// Retained CoreVideo Metal texture cache.
        CVMetalTextureCache, CVMetalTextureCacheRef
    }
    impl_TCFType!(
        CVMetalTextureCache,
        CVMetalTextureCacheRef,
        CVMetalTextureCacheGetTypeID
    );
    impl_CFTypeDescription!(CVMetalTextureCache);

    impl CVMetalTextureCache {
        /// # Safety
        ///
        /// `metal_device` must point to a live Objective-C object that conforms
        /// to `MTLDevice`. The object must use an ABI compatible with Apple's
        /// Metal framework and remain alive for this call; CoreVideo retains
        /// anything it needs after the function returns. A null pointer is
        /// rejected as an error.
        pub unsafe fn new(metal_device: *const MTLDevice) -> Result<Self> {
            anyhow::ensure!(!metal_device.is_null(), "Metal device pointer is null");
            let mut this = ptr::null();
            // SAFETY: the caller guarantees the raw Metal-device contract. The
            // optional attribute pointers may be null and `this` is writable.
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
            // SAFETY: a successful create call returned a non-null object with
            // ownership transferred under Core Foundation's create rule.
            unsafe { Ok(CVMetalTextureCache::wrap_under_create_rule(this)) }
        }

        /// Create a Metal texture backed by a plane of the `source` pixel buffer.
        ///
        /// `pixel_format`, `width`, `height`, and `plane_index` must describe a
        /// valid plane of the image buffer. CoreVideo reports incompatible
        /// combinations as an error.
        pub fn create_texture_from_image(
            &self,
            source: &CVPixelBuffer,
            texture_attributes: Option<&CFDictionary>,
            pixel_format: MTLPixelFormat,
            width: usize,
            height: usize,
            plane_index: usize,
        ) -> Result<CVMetalTexture> {
            let mut this = ptr::null();
            let source = source.as_concrete_TypeRef();
            let texture_attributes = texture_attributes
                .map(TCFType::as_concrete_TypeRef)
                .unwrap_or(ptr::null());
            // SAFETY: `self` and `source` are live retained Core Foundation
            // objects, optional attributes are either null or a live dictionary,
            // and `this` is writable. CoreVideo validates the plane parameters.
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
            // SAFETY: a successful create call returned a non-null object with
            // ownership transferred under Core Foundation's create rule.
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
            source_image: CVPixelBufferRef,
            texture_attributes: CFDictionaryRef,
            pixel_format: MTLPixelFormat,
            width: usize,
            height: usize,
            plane_index: usize,
            texture_out: *mut CVMetalTextureRef,
        ) -> CVReturn;
    }

    #[doc(hidden)]
    #[repr(C)]
    pub struct __CVMetalTexture(c_void);
    /// Borrowed CoreVideo Metal texture pointer.
    pub type CVMetalTextureRef = *const __CVMetalTexture;

    declare_TCFType! {
        /// Retained CoreVideo Metal texture.
        CVMetalTexture, CVMetalTextureRef
    }
    impl_TCFType!(CVMetalTexture, CVMetalTextureRef, CVMetalTextureGetTypeID);
    impl_CFTypeDescription!(CVMetalTexture);

    impl CVMetalTexture {
        /// Borrow the Metal texture backing this CoreVideo texture, if present.
        pub fn as_texture_ref(&self) -> Option<&metal::TextureRef> {
            // SAFETY: `self` owns a valid CoreVideo texture. The returned Metal
            // object is borrowed from it, checked for null, and the reference is
            // bounded by the borrow of self.
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

    #[cfg(test)]
    mod tests {
        use super::CVMetalTextureCache;

        #[test]
        fn texture_cache_rejects_null_device_before_ffi() {
            // SAFETY: null is explicitly accepted as an error case and is
            // rejected before the implementation calls CoreVideo.
            let result = unsafe { CVMetalTextureCache::new(std::ptr::null()) };
            assert!(result.is_err());
        }
    }
}

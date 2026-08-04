//! Minimal stable CoreMedia ABI definitions used by `kael_media_sys`.

#![cfg(target_os = "macos")]

use core_foundation::string::CFStringRef;

/// Signed index type used by CoreMedia sample buffers.
pub type CMItemIndex = isize;

/// Integer time value used by [`CMTime`].
pub type CMTimeValue = i64;

/// Time scale used by [`CMTime`].
pub type CMTimeScale = i32;

/// Timeline epoch used by [`CMTime`].
pub type CMTimeEpoch = i64;

/// Flags describing the validity and special values of a [`CMTime`].
pub type CMTimeFlags = u32;

/// A rational CoreMedia timestamp.
#[repr(C, packed(4))]
#[derive(Clone, Copy, Debug)]
#[allow(non_snake_case)]
pub struct CMTime {
    /// Numerator of the rational timestamp.
    pub value: CMTimeValue,
    /// Denominator of the rational timestamp.
    pub timescale: CMTimeScale,
    /// Validity and special-value flags.
    pub flags: CMTimeFlags,
    /// Timeline epoch.
    pub epoch: CMTimeEpoch,
}

/// The invalid CoreMedia time value.
#[allow(non_upper_case_globals)]
pub const kCMTimeInvalid: CMTime = CMTime {
    value: 0,
    timescale: 0,
    flags: 0,
    epoch: 0,
};

/// Create a CoreMedia time from a value and time scale.
#[allow(non_snake_case)]
pub fn CMTimeMake(value: CMTimeValue, timescale: CMTimeScale) -> CMTime {
    // SAFETY: `CMTimeMake` takes only integer values and returns a `CMTime` by value.
    unsafe { CMTimeMakeRaw(value, timescale) }
}

/// Four-character CoreMedia video codec identifier.
pub type CMVideoCodecType = u32;

/// H.264/AVC video codec identifier (`avc1`).
#[allow(non_upper_case_globals)]
pub const kCMVideoCodecType_H264: CMVideoCodecType = 0x6176_6331;

/// Timing information for one sample in a CoreMedia sample buffer.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
#[allow(non_snake_case)]
pub struct CMSampleTimingInfo {
    /// Duration of the sample.
    pub duration: CMTime,
    /// Presentation timestamp of the sample.
    pub presentationTimeStamp: CMTime,
    /// Decode timestamp of the sample.
    pub decodeTimeStamp: CMTime,
}

pub(crate) fn sample_attachment_key_not_sync_ref() -> CFStringRef {
    // SAFETY: CoreMedia exposes this process-lifetime constant as a non-null CFString.
    unsafe { kCMSampleAttachmentKey_NotSync }
}

#[link(name = "CoreMedia", kind = "framework")]
unsafe extern "C" {
    #[link_name = "CMTimeMake"]
    fn CMTimeMakeRaw(value: CMTimeValue, timescale: CMTimeScale) -> CMTime;
    static kCMSampleAttachmentKey_NotSync: CFStringRef;
}

#[cfg(test)]
mod tests {
    use super::{CMSampleTimingInfo, CMTime, CMTimeMake};
    use std::mem::{align_of, size_of};

    #[test]
    fn core_media_time_layout_matches_apple_abi() {
        assert_eq!(size_of::<CMTime>(), 24);
        assert_eq!(align_of::<CMTime>(), 4);
        assert_eq!(size_of::<CMSampleTimingInfo>(), 72);
        assert_eq!(align_of::<CMSampleTimingInfo>(), 4);
    }

    #[test]
    fn core_media_time_constructor_links_and_sets_valid_time() {
        let time = CMTimeMake(3, 60);
        let value = time.value;
        let timescale = time.timescale;
        let flags = time.flags;
        let epoch = time.epoch;

        assert_eq!(value, 3);
        assert_eq!(timescale, 60);
        assert_eq!(flags, 1);
        assert_eq!(epoch, 0);
    }
}

use crate::{DevicePixels, Pixels, Result, SharedString, Size, size};
use smallvec::SmallVec;

use image::{Delay, Frame, ImageFormat, ImageReader, Limits};
use std::{
    borrow::Cow,
    fmt,
    hash::Hash,
    io::Cursor,
    sync::atomic::{AtomicUsize, Ordering::SeqCst},
};

pub(crate) const MAX_IMAGE_SOURCE_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_IMAGE_DIMENSION: u32 = 16_384;
pub(crate) const MAX_DECODED_IMAGE_BYTES: usize = 256 * 1024 * 1024;
pub(crate) const MAX_IMAGE_ANIMATION_FRAMES: usize = 10_000;

pub(crate) fn validate_image_source_len(len: usize) -> Result<()> {
    anyhow::ensure!(len > 0, "image source cannot be empty");
    anyhow::ensure!(
        len <= MAX_IMAGE_SOURCE_BYTES,
        "image source cannot exceed {MAX_IMAGE_SOURCE_BYTES} bytes"
    );
    Ok(())
}

pub(crate) fn validate_image_source_bytes(bytes: &[u8]) -> Result<()> {
    validate_image_source_len(bytes.len())
}

pub(crate) fn image_decode_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_DECODED_IMAGE_BYTES as u64);
    limits
}

pub(crate) fn checked_image_frame_len(width: u32, height: u32) -> Result<usize> {
    anyhow::ensure!(
        width > 0 && height > 0 && width <= MAX_IMAGE_DIMENSION && height <= MAX_IMAGE_DIMENSION,
        "image dimensions must be between 1 and {MAX_IMAGE_DIMENSION} pixels"
    );
    let byte_len = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| anyhow::anyhow!("image frame dimensions overflowed"))?;
    anyhow::ensure!(
        byte_len <= MAX_DECODED_IMAGE_BYTES,
        "decoded image data cannot exceed {MAX_DECODED_IMAGE_BYTES} bytes"
    );
    Ok(byte_len)
}

pub(crate) fn decode_static_image(
    bytes: &[u8],
    format: ImageFormat,
) -> Result<SmallVec<[Frame; 1]>> {
    validate_image_source_bytes(bytes)?;
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(image_decode_limits());
    let image = reader.decode()?;
    checked_image_frame_len(image.width(), image.height())?;
    let mut data = image.into_rgba8();
    rgba_to_bgra(data.as_mut());
    Ok(SmallVec::from_elem(Frame::new(data), 1))
}

pub(crate) fn collect_animation_frames(
    frames: impl IntoIterator<Item = image::ImageResult<Frame>>,
) -> Result<SmallVec<[Frame; 1]>> {
    let mut decoded_bytes = 0usize;
    let mut decoded_frames = SmallVec::new();

    for frame in frames {
        anyhow::ensure!(
            decoded_frames.len() < MAX_IMAGE_ANIMATION_FRAMES,
            "animated images cannot exceed {MAX_IMAGE_ANIMATION_FRAMES} frames"
        );
        let mut frame = frame?;
        let buffer = frame.buffer_mut();
        let frame_bytes = checked_image_frame_len(buffer.width(), buffer.height())?;
        anyhow::ensure!(
            buffer.as_raw().len() == frame_bytes,
            "decoded image frame buffer has an invalid length"
        );
        decoded_bytes = decoded_bytes
            .checked_add(frame_bytes)
            .ok_or_else(|| anyhow::anyhow!("decoded image data length overflowed"))?;
        anyhow::ensure!(
            decoded_bytes <= MAX_DECODED_IMAGE_BYTES,
            "decoded image data cannot exceed {MAX_DECODED_IMAGE_BYTES} bytes"
        );
        rgba_to_bgra(buffer.as_mut());
        decoded_frames.push(frame);
    }

    anyhow::ensure!(!decoded_frames.is_empty(), "animated image has no frames");
    Ok(decoded_frames)
}

fn rgba_to_bgra(bytes: &mut [u8]) {
    for pixel in bytes.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
}

/// A source of assets for this app to use.
pub trait AssetSource: 'static + Send + Sync {
    /// Load the given asset from the source path.
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>>;

    /// List the assets at the given path.
    fn list(&self, path: &str) -> Result<Vec<SharedString>>;
}

impl AssetSource for () {
    fn load(&self, _path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(None)
    }

    fn list(&self, _path: &str) -> Result<Vec<SharedString>> {
        Ok(vec![])
    }
}

/// A unique identifier for the image cache
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ImageId(pub usize);

#[derive(PartialEq, Eq, Hash, Clone)]
pub(crate) struct RenderImageParams {
    pub(crate) image_id: ImageId,
    pub(crate) frame_index: usize,
}

/// A cached and processed image, in BGRA format
pub struct RenderImage {
    /// The ID associated with this image
    pub id: ImageId,
    /// The scale factor of this image on render.
    pub(crate) scale_factor: f32,
    data: SmallVec<[Frame; 1]>,
}

impl PartialEq for RenderImage {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for RenderImage {}

impl RenderImage {
    /// Create a new image from the given data.
    pub fn new(data: impl Into<SmallVec<[Frame; 1]>>) -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let id = NEXT_ID
            .fetch_update(SeqCst, SeqCst, |current| current.checked_add(1))
            .expect("render image identifier space exhausted");

        Self {
            id: ImageId(id),
            scale_factor: 1.0,
            data: data.into(),
        }
    }

    /// Convert this image into a byte slice.
    pub fn as_bytes(&self, frame_index: usize) -> Option<&[u8]> {
        self.data
            .get(frame_index)
            .map(|frame| frame.buffer().as_raw().as_slice())
    }

    /// Get the size of this image, in pixels.
    pub fn size(&self, frame_index: usize) -> Size<DevicePixels> {
        let (width, height) = self.data[frame_index].buffer().dimensions();
        size(width.into(), height.into())
    }

    /// Get the size of this image, in pixels for display, adjusted for the scale factor.
    pub(crate) fn render_size(&self, frame_index: usize) -> Size<Pixels> {
        self.size(frame_index)
            .map(|v| (v.0 as f32 / self.scale_factor).into())
    }

    /// Get the delay of this frame from the previous
    pub fn delay(&self, frame_index: usize) -> Delay {
        self.data[frame_index].delay()
    }

    /// Get the number of frames for this image.
    pub fn frame_count(&self) -> usize {
        self.data.len()
    }
}

impl fmt::Debug for RenderImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImageData")
            .field("id", &self.id)
            .field("size", &self.size(0))
            .finish()
    }
}

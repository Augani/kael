#[cfg(not(feature = "images"))]
use std::mem::size_of_val;
use std::sync::Arc;
#[cfg(feature = "images")]
use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    io::Cursor,
};

#[cfg(feature = "images")]
use base64::{Engine, engine::general_purpose};
#[cfg(feature = "images")]
use image::{
    ImageDecoder as _, ImageReader,
    imageops::{self, FilterType},
};
#[cfg(feature = "images")]
use rustc_hash::FxHashMap;
#[cfg(not(feature = "images"))]
use tiny_skia::Pixmap;
#[cfg(feature = "images")]
use tiny_skia::{IntSize, Pixmap};

use super::renderer::{ImageAssetResolver, ImageLimits};
#[cfg(feature = "images")]
use crate::Asset;
use crate::{Animation, RasterlottieError};

#[cfg(all(test, feature = "images"))]
thread_local! {
    static PIXEL_DECODES: Cell<usize> = const { Cell::new(0) };
    static RESIZES: Cell<usize> = const { Cell::new(0) };
}

#[cfg(feature = "images")]
#[derive(Debug, Default)]
pub(super) struct ImageAssetStore {
    entries: FxHashMap<String, LazyImageAssetEntry>,
    limits: ImageLimits,
    decoded_bytes: Cell<u64>,
}

#[cfg(not(feature = "images"))]
#[derive(Debug, Default)]
pub(super) struct ImageAssetStore;

#[cfg(feature = "images")]
#[derive(Debug)]
struct LazyImageAssetEntry {
    asset_index: usize,
    source: EncodedImageSource,
    decoded: RefCell<Option<Arc<Pixmap>>>,
}

#[cfg(feature = "images")]
struct DecodedReservation<'a> {
    total: &'a Cell<u64>,
    bytes: u64,
    committed: bool,
}

#[cfg(feature = "images")]
impl Drop for DecodedReservation<'_> {
    fn drop(&mut self) {
        if !self.committed {
            self.total.set(self.total.get() - self.bytes);
        }
    }
}

#[cfg(feature = "images")]
#[derive(Debug)]
enum EncodedImageSource {
    EmbeddedDataUrl,
    EncodedBytes(Vec<u8>),
}

#[cfg(feature = "images")]
impl Clone for EncodedImageSource {
    fn clone(&self) -> Self {
        match self {
            Self::EmbeddedDataUrl => Self::EmbeddedDataUrl,
            Self::EncodedBytes(bytes) => Self::EncodedBytes(bytes.clone()),
        }
    }
}

impl ImageAssetStore {
    #[cfg(all(feature = "images", feature = "gif"))]
    pub(super) fn clone_for_worker(&self) -> Self {
        let mut entries = FxHashMap::default();
        entries.reserve(self.entries.len());
        for (id, entry) in &self.entries {
            entries.insert(
                id.clone(),
                LazyImageAssetEntry {
                    asset_index: entry.asset_index,
                    source: entry.source.clone(),
                    decoded: RefCell::default(),
                },
            );
        }

        Self {
            entries,
            limits: self.limits,
            decoded_bytes: Cell::new(0),
        }
    }

    #[cfg(all(not(feature = "images"), feature = "gif"))]
    pub(super) const fn clone_for_worker(&self) -> Self {
        let _store_size = size_of_val(self);
        Self
    }

    #[cfg(feature = "images")]
    pub(super) fn get(
        &self,
        animation: &Animation,
        ref_id: &str,
    ) -> Result<Option<Arc<Pixmap>>, RasterlottieError> {
        let Some(entry) = self.entries.get(ref_id) else {
            return Ok(None);
        };

        if let Some(decoded) = entry.decoded.borrow().as_ref().cloned() {
            return Ok(Some(decoded));
        }

        let asset = animation.assets.get(entry.asset_index).ok_or_else(|| {
            RasterlottieError::InvalidImageAsset {
                id: ref_id.to_string(),
                detail: "image asset index is out of bounds".to_string(),
            }
        })?;
        validate_declared_dimensions(asset, self.limits)?;
        let bytes = match &entry.source {
            EncodedImageSource::EmbeddedDataUrl => {
                let data_url =
                    asset
                        .image_data_url()
                        .ok_or_else(|| RasterlottieError::InvalidImageAsset {
                            id: asset.id.clone(),
                            detail: "embedded image asset is missing a data URL".to_string(),
                        })?;
                Cow::Owned(decode_data_url_bytes(asset, data_url, self.limits)?)
            }
            EncodedImageSource::EncodedBytes(bytes) => Cow::Borrowed(bytes.as_slice()),
        };
        let decoded = Arc::new(decode_image_bytes(
            asset,
            &bytes,
            self.limits,
            &self.decoded_bytes,
        )?);
        *entry.decoded.borrow_mut() = Some(Arc::clone(&decoded));
        Ok(Some(decoded))
    }

    #[cfg(not(feature = "images"))]
    pub(super) fn get(
        &self,
        _animation: &Animation,
        ref_id: &str,
    ) -> Result<Option<Arc<Pixmap>>, RasterlottieError> {
        let _store_size = size_of_val(self);
        if ref_id.is_empty() {
            Ok(None)
        } else {
            Err(RasterlottieError::InvalidImageAsset {
                id: ref_id.to_string(),
                detail: "image support is disabled because the `images` feature is not enabled"
                    .to_string(),
            })
        }
    }
}

#[cfg(feature = "images")]
pub(super) fn resolve_image_assets(
    animation: &Animation,
    resolver: Option<&dyn ImageAssetResolver>,
    limits: ImageLimits,
) -> Result<ImageAssetStore, RasterlottieError> {
    let mut entries = FxHashMap::default();
    let mut resolved_bytes = 0u64;
    for (asset_index, asset) in animation.assets.iter().enumerate() {
        if !asset.is_image_asset() {
            continue;
        }

        let source = if asset.image_data_url().is_some() {
            EncodedImageSource::EmbeddedDataUrl
        } else if let Some(resolver) = resolver {
            let bytes = resolver.resolve_image_asset(asset)?.ok_or_else(|| {
                RasterlottieError::InvalidImageAsset {
                    id: asset.id.clone(),
                    detail: "image asset resolver did not return bytes".to_string(),
                }
            })?;
            if bytes.len() as u64 > limits.max_encoded_bytes {
                return Err(image_limit_error(asset, "resolver encoded image bytes"));
            }
            resolved_bytes = resolved_bytes
                .checked_add(bytes.len() as u64)
                .filter(|total| *total <= limits.max_total_resolved_encoded_bytes)
                .ok_or_else(|| {
                    image_limit_error(asset, "aggregate resolver encoded image bytes")
                })?;
            EncodedImageSource::EncodedBytes(bytes)
        } else {
            return Err(RasterlottieError::InvalidImageAsset {
                id: asset.id.clone(),
                detail: "only embedded data URL image assets are supported without a resolver"
                    .to_string(),
            });
        };

        entries.insert(
            asset.id.clone(),
            LazyImageAssetEntry {
                asset_index,
                source,
                decoded: RefCell::default(),
            },
        );
    }

    Ok(ImageAssetStore {
        entries,
        limits,
        decoded_bytes: Cell::new(0),
    })
}

#[cfg(not(feature = "images"))]
pub(super) fn resolve_image_assets(
    animation: &Animation,
    resolver: Option<&dyn ImageAssetResolver>,
    _limits: ImageLimits,
) -> Result<ImageAssetStore, RasterlottieError> {
    if let Some(asset) = animation.assets.iter().find(|asset| asset.is_image_asset()) {
        return Err(RasterlottieError::InvalidImageAsset {
            id: asset.id.clone(),
            detail: "image support is disabled because the `images` feature is not enabled"
                .to_string(),
        });
    }

    if resolver.is_some() {
        return Err(RasterlottieError::InvalidImageAsset {
            id: "<resolver>".to_string(),
            detail: "image support is disabled because the `images` feature is not enabled"
                .to_string(),
        });
    }

    Ok(ImageAssetStore)
}

#[cfg(feature = "images")]
fn image_limit_error(asset: &Asset, detail: &str) -> RasterlottieError {
    RasterlottieError::InvalidImageAsset {
        id: asset.id.clone(),
        detail: format!("image admission limit exceeded: {detail}"),
    }
}

#[cfg(feature = "images")]
fn fits_buffer(len: u64) -> bool {
    // Vec/ImageBuffer requires isize representability, not merely usize. This
    // matters on wasm32 even when a caller explicitly raises admission limits.
    len <= isize::MAX as u64
}

#[cfg(feature = "images")]
fn validate_declared_dimensions(
    asset: &Asset,
    limits: ImageLimits,
) -> Result<(), RasterlottieError> {
    for dimension in [asset.width, asset.height].into_iter().flatten() {
        if dimension == 0 || dimension > limits.max_dimension {
            return Err(image_limit_error(asset, "declared target dimension"));
        }
    }
    Ok(())
}

#[cfg(feature = "images")]
fn rgba_len(
    asset: &Asset,
    width: u32,
    height: u32,
    limits: ImageLimits,
) -> Result<u64, RasterlottieError> {
    if width == 0 || height == 0 || width > limits.max_dimension || height > limits.max_dimension {
        return Err(image_limit_error(asset, "source or target dimension"));
    }
    let len = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|len| len.checked_mul(4))
        .ok_or_else(|| image_limit_error(asset, "RGBA size overflow"))?;
    if len > limits.max_rgba_bytes || !fits_buffer(len) {
        return Err(image_limit_error(asset, "source or target RGBA bytes"));
    }
    Ok(len)
}

#[cfg(feature = "images")]
fn decode_image_bytes(
    asset: &Asset,
    bytes: &[u8],
    limits: ImageLimits,
    retained_bytes: &Cell<u64>,
) -> Result<Pixmap, RasterlottieError> {
    validate_declared_dimensions(asset, limits)?;
    if bytes.len() as u64 > limits.max_encoded_bytes {
        return Err(image_limit_error(asset, "encoded image bytes"));
    }
    let decode_error = |error| RasterlottieError::InvalidImageAsset {
        id: asset.id.clone(),
        detail: format!("failed to decode image bytes: {error}"),
    };
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| RasterlottieError::InvalidImageAsset {
            id: asset.id.clone(),
            detail: error.to_string(),
        })?;
    let mut decoder_limits = image::Limits::default();
    decoder_limits.max_image_width = Some(limits.max_dimension);
    decoder_limits.max_image_height = Some(limits.max_dimension);
    decoder_limits.max_alloc = Some(limits.max_working_bytes);
    reader.limits(decoder_limits);
    let decoder = reader.into_decoder().map_err(&decode_error)?;
    let (source_width, source_height) = decoder.dimensions();
    let source_len = rgba_len(asset, source_width, source_height, limits)?;
    let target_width = asset.width.unwrap_or(source_width);
    let target_height = asset.height.unwrap_or(source_height);
    let target_len = rgba_len(asset, target_width, target_height, limits)?;
    if retained_bytes
        .get()
        .checked_add(target_len)
        .is_none_or(|len| len > limits.max_total_decoded_bytes)
    {
        return Err(image_limit_error(asset, "aggregate decoded image bytes"));
    }
    let decoded_len = decoder.total_bytes();
    if decoded_len > limits.max_decode_bytes || !fits_buffer(decoded_len) {
        return Err(image_limit_error(asset, "native decoded image bytes"));
    }
    let resizing = (source_width, source_height) != (target_width, target_height);
    // image::imageops::resize(Triangle) first allocates an RGBA32F intermediate
    // with source width and target height, even when source/target RGBA8 are tiny.
    let scratch_len = if resizing {
        u64::from(source_width)
            .checked_mul(u64::from(target_height))
            .and_then(|len| len.checked_mul(16))
            .ok_or_else(|| image_limit_error(asset, "Triangle scratch size overflow"))?
    } else {
        0
    };
    // Each sampling pass grows a Vec<f32> with at most one weight per source
    // row/column. Include power-of-two growth (minimum capacity four), and
    // conservatively charge both vectors even though the passes are sequential.
    let weights_len = if resizing {
        [source_width, source_height]
            .into_iter()
            .try_fold(0u64, |sum, count| {
                u64::from(count)
                    .max(4)
                    .checked_next_power_of_two()
                    .and_then(|capacity| capacity.checked_mul(4))
                    .and_then(|len| sum.checked_add(len))
            })
            .ok_or_else(|| image_limit_error(asset, "Triangle weight size overflow"))?
    } else {
        0
    };
    let working = decoded_len
        .checked_add(source_len)
        .and_then(|len| len.checked_add(if resizing { target_len } else { 0 }))
        .and_then(|len| len.checked_add(scratch_len))
        .and_then(|len| len.checked_add(weights_len));
    if working.is_none_or(|len| len > limits.max_working_bytes || !fits_buffer(len)) {
        return Err(image_limit_error(
            asset,
            "decode/conversion/Triangle working bytes",
        ));
    }
    let total = retained_bytes
        .get()
        .checked_add(target_len)
        .filter(|len| *len <= limits.max_total_decoded_bytes)
        .ok_or_else(|| image_limit_error(asset, "aggregate decoded image bytes"))?;
    retained_bytes.set(total);
    let mut reservation = DecodedReservation {
        total: retained_bytes,
        bytes: target_len,
        committed: false,
    };
    #[cfg(test)]
    PIXEL_DECODES.with(|count| count.set(count.get() + 1));
    let mut rgba = image::DynamicImage::from_decoder(decoder)
        .map_err(decode_error)?
        .into_rgba8();
    if resizing {
        #[cfg(test)]
        RESIZES.with(|count| count.set(count.get() + 1));
        rgba = imageops::resize(&rgba, target_width, target_height, FilterType::Triangle);
    }
    premultiply_rgba(rgba.as_mut());
    let size = IntSize::from_wh(target_width, target_height)
        .ok_or_else(|| image_limit_error(asset, "invalid target dimensions"))?;
    let pixmap = Pixmap::from_vec(rgba.into_vec(), size)
        .ok_or_else(|| image_limit_error(asset, "invalid target buffer"))?;
    reservation.committed = true;
    Ok(pixmap)
}

#[cfg(feature = "images")]
fn decode_data_url_bytes(
    asset: &Asset,
    data_url: &str,
    limits: ImageLimits,
) -> Result<Vec<u8>, RasterlottieError> {
    let Some((metadata, payload)) = data_url.split_once(',') else {
        return Err(RasterlottieError::InvalidImageAsset {
            id: asset.id.clone(),
            detail: "embedded image asset is not a valid data URL".to_string(),
        });
    };
    if !metadata.starts_with("data:") {
        return Err(RasterlottieError::InvalidImageAsset {
            id: asset.id.clone(),
            detail: "embedded image asset is not a data URL".to_string(),
        });
    }
    if !metadata.contains(";base64") {
        return Err(RasterlottieError::InvalidImageAsset {
            id: asset.id.clone(),
            detail: "only base64-encoded data URLs are supported".to_string(),
        });
    }

    let payload = payload.trim();
    // STANDARD requires padded blocks. Compute the exact valid decoded length
    // before allocating, so a limit-boundary image is accepted without a +2 guess.
    if payload.len() % 4 != 0 {
        return Err(image_limit_error(asset, "invalid base64 block length"));
    }
    let padding = payload
        .as_bytes()
        .iter()
        .rev()
        .take_while(|byte| **byte == b'=')
        .count();
    if padding > 2 {
        return Err(image_limit_error(asset, "invalid base64 padding"));
    }
    let decoded_len = (payload.len() / 4)
        .checked_mul(3)
        .and_then(|len| len.checked_sub(padding))
        .ok_or_else(|| image_limit_error(asset, "base64 size overflow"))?;
    if decoded_len as u64 > limits.max_encoded_bytes || !fits_buffer(decoded_len as u64) {
        return Err(image_limit_error(asset, "base64 decoded image bytes"));
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(decoded_len)
        .map_err(|_| image_limit_error(asset, "cannot reserve bounded base64 buffer"))?;
    bytes.resize(decoded_len, 0);
    let count = general_purpose::STANDARD
        .decode_slice(payload, &mut bytes)
        .map_err(|error| RasterlottieError::InvalidImageAsset {
            id: asset.id.clone(),
            detail: format!("failed to decode base64 image data: {error}"),
        })?;
    bytes.truncate(count);
    Ok(bytes)
}

#[cfg(feature = "images")]
fn premultiply_rgba(pixels: &mut [u8]) {
    for pixel in pixels.chunks_exact_mut(4) {
        let alpha = pixel[3] as u16;
        pixel[0] = ((pixel[0] as u16 * alpha + 127) / 255) as u8;
        pixel[1] = ((pixel[1] as u16 * alpha + 127) / 255) as u8;
        pixel[2] = ((pixel[2] as u16 * alpha + 127) / 255) as u8;
    }
}

#[cfg(all(test, feature = "images"))]
mod tests {
    use super::*;
    use image::{ColorType, ImageEncoder as _, codecs::png::PngEncoder};

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        PngEncoder::new(&mut bytes)
            .write_image(
                &[255, 0, 0, 255].repeat(width as usize * height as usize),
                width,
                height,
                ColorType::Rgba8.into(),
            )
            .unwrap();
        bytes
    }

    fn animation() -> Animation {
        let data = general_purpose::STANDARD.encode(png(1, 1));
        Animation::from_json_str(&format!(r#"{{"fr":30,"ip":0,"op":1,"w":1,"h":1,
            "assets":[{{"id":"first","w":1,"h":1,"p":"data:image/png;base64,{data}","e":1}},
                      {{"id":"second","w":1,"h":1,"p":"data:image/png;base64,{data}","e":1}}],"layers":[]}}"#)).unwrap()
    }

    fn reset_work() {
        PIXEL_DECODES.with(|count| count.set(0));
        RESIZES.with(|count| count.set(0));
    }

    fn assert_no_work() {
        assert_eq!(PIXEL_DECODES.with(Cell::get), 0);
        assert_eq!(RESIZES.with(Cell::get), 0);
    }

    #[test]
    fn admission_checks_encoded_native_rgba_target_and_aggregate_before_pixels() {
        let animation = animation();
        let asset = &animation.assets[0];
        let bytes = png(1, 1);
        for limits in [
            ImageLimits {
                max_encoded_bytes: bytes.len() as u64 - 1,
                ..ImageLimits::default()
            },
            ImageLimits {
                max_decode_bytes: 3,
                ..ImageLimits::default()
            },
            ImageLimits {
                max_rgba_bytes: 3,
                ..ImageLimits::default()
            },
            ImageLimits {
                max_total_decoded_bytes: 3,
                ..ImageLimits::default()
            },
            ImageLimits {
                max_dimension: 0,
                ..ImageLimits::default()
            },
        ] {
            reset_work();
            assert!(decode_image_bytes(asset, &bytes, limits, &Cell::new(0)).is_err());
            assert_no_work();
        }
        reset_work();
        assert!(
            decode_image_bytes(asset, &bytes, ImageLimits::default(), &Cell::new(u64::MAX))
                .is_err()
        );
        assert_no_work();
        let mut target = asset.clone();
        target.width = Some(8193);
        let error = decode_image_bytes(
            &target,
            b"not an image",
            ImageLimits::default(),
            &Cell::new(0),
        )
        .unwrap_err();
        assert!(error.to_string().contains("declared target"));
        assert_no_work();
        let mut limits = ImageLimits::default();
        limits.max_encoded_bytes = bytes.len() as u64;
        limits.max_decode_bytes = 4;
        limits.max_rgba_bytes = 4;
        limits.max_total_decoded_bytes = 4;
        assert_eq!(
            decode_image_bytes(asset, &bytes, limits, &Cell::new(0))
                .unwrap()
                .data(),
            [255, 0, 0, 255]
        );
        assert_eq!(PIXEL_DECODES.with(Cell::get), 1);
        assert_eq!(RESIZES.with(Cell::get), 0);
    }

    #[test]
    fn admission_checks_actual_header_and_triangle_cross_aspect_scratch() {
        let animation = animation();
        let mut asset = animation.assets[0].clone();
        asset.width = Some(1);
        asset.height = Some(256);
        let bytes = png(256, 1);
        let limits = ImageLimits {
            max_working_bytes: 128 * 1024,
            ..ImageLimits::default()
        };
        reset_work();
        let error = decode_image_bytes(&asset, &bytes, limits, &Cell::new(0)).unwrap_err();
        assert!(error.to_string().contains("working bytes"));
        assert_no_work();
        asset.width = Some(1);
        asset.height = Some(1);
        let limits = ImageLimits {
            max_dimension: 255,
            ..ImageLimits::default()
        };
        assert!(decode_image_bytes(&asset, &bytes, limits, &Cell::new(0)).is_err());
        assert_no_work();
        asset.width = Some(u32::MAX);
        asset.height = Some(u32::MAX);
        let limits = ImageLimits {
            max_dimension: u32::MAX,
            max_rgba_bytes: u64::MAX,
            ..ImageLimits::default()
        };
        assert!(decode_image_bytes(&asset, &png(1, 1), limits, &Cell::new(0)).is_err());
        assert_no_work();
        let cap = isize::MAX as u64;
        assert!(fits_buffer(cap));
        assert!(!fits_buffer(cap + 1));
    }

    #[test]
    fn admission_includes_triangle_weight_buffers() {
        let animation = animation();
        let mut asset = animation.assets[0].clone();
        asset.width = Some(1);
        asset.height = Some(1);
        let mut bytes = Vec::new();
        PngEncoder::new(&mut bytes)
            .write_image(&[255; 8192], 1, 8192, ColorType::L8.into())
            .unwrap();
        let mut limits = ImageLimits {
            max_working_bytes: 60 * 1024,
            ..ImageLimits::default()
        };
        reset_work();
        assert!(decode_image_bytes(&asset, &bytes, limits, &Cell::new(0)).is_err());
        assert_no_work();
        limits.max_working_bytes = 128 * 1024;
        assert_eq!(
            decode_image_bytes(&asset, &bytes, limits, &Cell::new(0))
                .unwrap()
                .data(),
            [255, 255, 255, 255]
        );
        assert_eq!(PIXEL_DECODES.with(Cell::get), 1);
        assert_eq!(RESIZES.with(Cell::get), 1);
    }

    #[test]
    fn admission_cache_hits_and_failures_preserve_exact_retained_accounting() {
        let mut animation = animation();
        let limits = ImageLimits {
            max_total_decoded_bytes: 4,
            ..ImageLimits::default()
        };
        let store = resolve_image_assets(&animation, None, limits).unwrap();
        reset_work();
        let first = store.get(&animation, "first").unwrap().unwrap();
        let hit = store.get(&animation, "first").unwrap().unwrap();
        assert!(Arc::ptr_eq(&first, &hit));
        assert_eq!(store.decoded_bytes.get(), 4);
        assert_eq!(PIXEL_DECODES.with(Cell::get), 1);
        assert!(store.get(&animation, "second").is_err());
        assert_eq!(store.decoded_bytes.get(), 4);
        assert!(store.entries["second"].decoded.borrow().is_none());
        assert_eq!(PIXEL_DECODES.with(Cell::get), 1);
        animation.assets[0].path = Some("data:image/png;base64,bm90IHBpeGVscw==".to_string());
        let store = resolve_image_assets(&animation, None, limits).unwrap();
        assert!(store.get(&animation, "first").is_err());
        assert_eq!(store.decoded_bytes.get(), 0);
        assert!(store.get(&animation, "second").unwrap().is_some());
        assert_eq!(store.decoded_bytes.get(), 4);
    }

    #[test]
    fn admission_rolls_back_after_pixel_decode_failure() {
        let animation = animation();
        let mut bytes = png(1, 1);
        let idat = bytes.windows(4).position(|part| part == b"IDAT").unwrap();
        bytes[idat + 4] ^= 0x80;
        let retained = Cell::new(4);
        let limits = ImageLimits {
            max_total_decoded_bytes: 8,
            ..ImageLimits::default()
        };
        reset_work();
        assert!(decode_image_bytes(&animation.assets[0], &bytes, limits, &retained).is_err());
        assert_eq!(PIXEL_DECODES.with(Cell::get), 1);
        assert_eq!(retained.get(), 4);
        assert_eq!(
            decode_image_bytes(&animation.assets[1], &png(1, 1), limits, &retained)
                .unwrap()
                .data(),
            [255, 0, 0, 255]
        );
        assert_eq!(retained.get(), 8);
    }

    #[test]
    fn admission_base64_boundary_and_declared_target_precede_decode() {
        let mut animation = animation();
        let bytes = png(1, 1);
        let mut limits = ImageLimits {
            max_encoded_bytes: bytes.len() as u64,
            ..ImageLimits::default()
        };
        let asset = &animation.assets[0];
        assert_eq!(
            decode_data_url_bytes(asset, asset.path.as_deref().unwrap(), limits).unwrap(),
            bytes
        );
        limits.max_encoded_bytes -= 1;
        assert!(decode_data_url_bytes(asset, asset.path.as_deref().unwrap(), limits).is_err());
        animation.assets[0].width = Some(8193);
        animation.assets[0].path = Some("not a data URL".to_string());
        let store = resolve_image_assets(&animation, None, ImageLimits::default()).unwrap();
        reset_work();
        let error = store.get(&animation, "first").unwrap_err();
        assert!(error.to_string().contains("declared target"));
        assert_eq!(store.decoded_bytes.get(), 0);
        assert_no_work();
    }

    #[test]
    fn admission_rejects_resolver_bytes_before_retention() {
        let mut animation = animation();
        for asset in &mut animation.assets {
            asset.embedded = None;
            asset.path = Some("pixel.png".to_string());
        }
        let bytes = png(1, 1);
        let resolver = |_: &Asset| Ok(Some(bytes.clone()));
        let mut limits = ImageLimits {
            max_encoded_bytes: bytes.len() as u64 - 1,
            ..ImageLimits::default()
        };
        assert!(
            resolve_image_assets(&animation, Some(&resolver), limits)
                .unwrap_err()
                .to_string()
                .contains("resolver encoded")
        );
        limits.max_encoded_bytes += 1;
        limits.max_total_resolved_encoded_bytes = bytes.len() as u64 * 2 - 1;
        assert!(
            resolve_image_assets(&animation, Some(&resolver), limits)
                .unwrap_err()
                .to_string()
                .contains("aggregate resolver")
        );
        limits.max_total_resolved_encoded_bytes += 1;
        let store = resolve_image_assets(&animation, Some(&resolver), limits).unwrap();
        assert_eq!(
            store.get(&animation, "first").unwrap().unwrap().data(),
            [255, 0, 0, 255]
        );
        #[cfg(feature = "gif")]
        {
            let worker = store.clone_for_worker();
            assert_eq!(worker.limits, limits);
            assert_eq!(worker.decoded_bytes.get(), 0);
            assert_eq!(
                worker.get(&animation, "second").unwrap().unwrap().data(),
                [255, 0, 0, 255]
            );
            assert_eq!(worker.decoded_bytes.get(), 4);
            assert_eq!(store.decoded_bytes.get(), 4);
        }
    }
}

#[cfg(feature = "images")]
use std::cell::RefCell;
#[cfg(not(feature = "images"))]
use std::mem::size_of_val;
use std::sync::Arc;

#[cfg(feature = "images")]
use base64::{Engine, engine::general_purpose};
#[cfg(feature = "images")]
use image::imageops::{self, FilterType};
#[cfg(feature = "images")]
use rustc_hash::FxHashMap;
#[cfg(not(feature = "images"))]
use tiny_skia::Pixmap;
#[cfg(feature = "images")]
use tiny_skia::{IntSize, Pixmap};

use super::renderer::ImageAssetResolver;
#[cfg(feature = "images")]
use crate::Asset;
use crate::{Animation, RasterlottieError};

#[cfg(feature = "images")]
#[derive(Debug, Default)]
pub(super) struct ImageAssetStore {
    entries: FxHashMap<String, LazyImageAssetEntry>,
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

        Self { entries }
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
        let bytes = match &entry.source {
            EncodedImageSource::EmbeddedDataUrl => {
                let data_url =
                    asset
                        .image_data_url()
                        .ok_or_else(|| RasterlottieError::InvalidImageAsset {
                            id: asset.id.clone(),
                            detail: "embedded image asset is missing a data URL".to_string(),
                        })?;
                decode_data_url_bytes(asset, data_url)?
            }
            EncodedImageSource::EncodedBytes(bytes) => bytes.clone(),
        };
        let decoded = Arc::new(decode_image_bytes(asset, &bytes)?);
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
) -> Result<ImageAssetStore, RasterlottieError> {
    let mut entries = FxHashMap::default();
    for (asset_index, asset) in animation.assets.iter().enumerate() {
        if !asset.is_image_asset() {
            continue;
        }

        let source = if asset.image_data_url().is_some() {
            EncodedImageSource::EmbeddedDataUrl
        } else if let Some(resolver) = resolver {
            EncodedImageSource::EncodedBytes(resolver.resolve_image_asset(asset)?.ok_or_else(
                || RasterlottieError::InvalidImageAsset {
                    id: asset.id.clone(),
                    detail: "image asset resolver did not return bytes".to_string(),
                },
            )?)
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

    Ok(ImageAssetStore { entries })
}

#[cfg(not(feature = "images"))]
pub(super) fn resolve_image_assets(
    animation: &Animation,
    resolver: Option<&dyn ImageAssetResolver>,
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
fn decode_image_bytes(asset: &Asset, bytes: &[u8]) -> Result<Pixmap, RasterlottieError> {
    let mut rgba = image::load_from_memory(bytes)
        .map_err(|err| RasterlottieError::InvalidImageAsset {
            id: asset.id.clone(),
            detail: format!("failed to decode image bytes: {err}"),
        })?
        .to_rgba8();

    let target_width = asset.width.unwrap_or_else(|| rgba.width());
    let target_height = asset.height.unwrap_or_else(|| rgba.height());
    if target_width == 0 || target_height == 0 {
        return Err(RasterlottieError::InvalidImageAsset {
            id: asset.id.clone(),
            detail: "image asset dimensions must be non-zero".to_string(),
        });
    }

    if rgba.width() != target_width || rgba.height() != target_height {
        rgba = imageops::resize(&rgba, target_width, target_height, FilterType::Triangle);
    }

    premultiply_rgba(rgba.as_mut());
    let size = IntSize::from_wh(target_width, target_height).ok_or(
        RasterlottieError::InvalidCanvasSize {
            width: target_width,
            height: target_height,
        },
    )?;
    Pixmap::from_vec(rgba.into_vec(), size).ok_or_else(|| RasterlottieError::InvalidImageAsset {
        id: asset.id.clone(),
        detail: "failed to create image pixmap".to_string(),
    })
}

#[cfg(feature = "images")]
fn decode_data_url_bytes(asset: &Asset, data_url: &str) -> Result<Vec<u8>, RasterlottieError> {
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

    general_purpose::STANDARD
        .decode(payload.trim())
        .map_err(|err| RasterlottieError::InvalidImageAsset {
            id: asset.id.clone(),
            detail: format!("failed to decode base64 image data: {err}"),
        })
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

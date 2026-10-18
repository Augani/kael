use std::{
    io::{Cursor, Read},
    path::Path,
};

#[cfg(feature = "images")]
use base64::{Engine, engine::general_purpose};
use serde::Deserialize;
use zip::{ZipArchive, read::ZipReadOptions, result::ZipError};

#[cfg(feature = "images")]
use crate::Asset;
use crate::{Animation, RasterlottieError};

#[cfg(test)]
thread_local! {
    static ALLOCATION_REQUESTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Debug, Deserialize)]
struct DotLottieManifest {
    #[serde(default, rename = "activeAnimationId")]
    active_animation_id: Option<String>,
    #[serde(default)]
    animations: Vec<DotLottieManifestAnimation>,
    #[serde(default)]
    initial: Option<DotLottieInitial>,
}

#[derive(Debug, Deserialize)]
struct DotLottieManifestAnimation {
    id: String,
}

#[derive(Debug, Deserialize)]
struct DotLottieInitial {
    #[serde(default)]
    animation: Option<String>,
}

/// Finite admission limits for loading a selected animation from a ZIP container.
///
/// Byte limits are inclusive. Zero rejects nonempty content. The total counts
/// bytes actually read for the manifest, selected JSON and each image embedding,
/// including repeated reads of the same entry. Base64 strings can occupy about
/// four thirds of the image bytes; parsed model and ZIP metadata are additional.
/// These are admission limits, not a process RSS or allocator limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct DotLottieLimits {
    /// Maximum compressed source bytes, checked before opening the ZIP archive.
    pub max_archive_bytes: u64,
    /// Maximum entry count after ZIP metadata construction. This does not bound
    /// the constructor's central-directory allocation. Only compressed source
    /// bytes are bounded before the ZIP constructor runs.
    pub max_entries: usize,
    /// Maximum expanded manifest bytes.
    pub max_manifest_bytes: u64,
    /// Maximum expanded selected animation JSON bytes.
    pub max_animation_bytes: u64,
    /// Maximum expanded encoded image bytes per embedding.
    pub max_image_bytes: u64,
    /// Maximum aggregate bytes read from entries actually used by the loader.
    pub max_total_expanded_bytes: u64,
}

impl Default for DotLottieLimits {
    fn default() -> Self {
        Self {
            max_archive_bytes: 16 * 1024 * 1024,
            max_entries: 4096,
            max_manifest_bytes: 1024 * 1024,
            max_animation_bytes: 16 * 1024 * 1024,
            max_image_bytes: 16 * 1024 * 1024,
            max_total_expanded_bytes: 64 * 1024 * 1024,
        }
    }
}

pub fn load_animation_from_dotlottie_bytes(
    dotlottie: &[u8],
) -> Result<Animation, RasterlottieError> {
    load_animation_from_dotlottie_bytes_with_limits(dotlottie, DotLottieLimits::default())
}

pub fn load_animation_from_dotlottie_bytes_with_limits(
    dotlottie: &[u8],
    limits: DotLottieLimits,
) -> Result<Animation, RasterlottieError> {
    if u64::try_from(dotlottie.len()).unwrap_or(u64::MAX) > limits.max_archive_bytes {
        return Err(admission_error(
            "archive",
            "compressed source limit exceeded",
        ));
    }
    let mut archive = ZipArchive::new(Cursor::new(dotlottie))?;
    if archive.len() > limits.max_entries {
        return Err(admission_error(
            "archive",
            "ZIP entry count limit exceeded after metadata parsing",
        ));
    }
    let mut remaining = limits.max_total_expanded_bytes;
    let manifest_json = read_archive_string_any(
        &mut archive,
        &["manifest.json".to_string()],
        limits.max_manifest_bytes,
        &mut remaining,
    )?;
    let manifest = serde_json::from_str::<DotLottieManifest>(&manifest_json).map_err(|error| {
        RasterlottieError::InvalidDotLottie {
            detail: format!("failed to parse manifest.json: {error}"),
        }
    })?;
    let animation_id = select_animation_id(&manifest)?;
    let animation_json = read_archive_string_any(
        &mut archive,
        &animation_candidate_paths(animation_id),
        limits.max_animation_bytes,
        &mut remaining,
    )?;
    #[cfg(feature = "images")]
    let mut animation = Animation::from_json_str(&animation_json)?;
    #[cfg(not(feature = "images"))]
    let animation = Animation::from_json_str(&animation_json)?;
    #[cfg(feature = "images")]
    embed_archive_images(
        &mut archive,
        &mut animation,
        limits.max_image_bytes,
        &mut remaining,
    )?;
    Ok(animation)
}

fn admission_error(path: &str, detail: &str) -> RasterlottieError {
    RasterlottieError::InvalidDotLottie {
        detail: format!("archive entry `{path}` exceeds admission limits: {detail}"),
    }
}

fn select_animation_id(manifest: &DotLottieManifest) -> Result<&str, RasterlottieError> {
    if let Some(initial) = manifest.initial.as_ref()
        && let Some(animation) = initial.animation.as_deref()
    {
        return Ok(animation);
    }
    if let Some(animation) = manifest.active_animation_id.as_deref() {
        return Ok(animation);
    }
    manifest
        .animations
        .first()
        .map(|animation| animation.id.as_str())
        .ok_or_else(|| RasterlottieError::InvalidDotLottie {
            detail: "manifest.json does not list any animations".to_string(),
        })
}

fn animation_candidate_paths(animation_id: &str) -> Vec<String> {
    let normalized = normalize_archive_path(animation_id);
    let mut candidates = Vec::new();
    if normalized.is_empty() {
        return candidates;
    }

    push_candidate(&mut candidates, normalized.clone());
    if path_uses_json_extension(&normalized) {
        push_candidate(&mut candidates, join_archive_path("a", &normalized));
        push_candidate(
            &mut candidates,
            join_archive_path("animations", &normalized),
        );
        if let Some(stem) = normalized.strip_suffix(".json") {
            push_candidate(
                &mut candidates,
                join_archive_path("a", &format!("{stem}.json")),
            );
            push_candidate(
                &mut candidates,
                join_archive_path("animations", &format!("{stem}.json")),
            );
        }
    } else {
        push_candidate(&mut candidates, format!("{normalized}.json"));
        push_candidate(
            &mut candidates,
            join_archive_path("a", &format!("{normalized}.json")),
        );
        push_candidate(
            &mut candidates,
            join_archive_path("animations", &format!("{normalized}.json")),
        );
    }

    candidates
}

fn read_archive_string_any(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    candidates: &[String],
    per_entry_limit: u64,
    remaining: &mut u64,
) -> Result<String, RasterlottieError> {
    let (path, bytes) =
        read_archive_bytes_optional(archive, candidates, per_entry_limit, remaining)?.ok_or_else(
            || RasterlottieError::InvalidDotLottie {
                detail: format!(
                    "archive is missing required entry; tried {}",
                    candidates.join(", ")
                ),
            },
        )?;
    String::from_utf8(bytes).map_err(|error| RasterlottieError::InvalidDotLottie {
        detail: format!("archive entry `{path}` is not valid UTF-8: {error}"),
    })
}

/// Reject declared oversize before any payload read or output allocation, then
/// enforce actual output as well. A forged header never authorizes extra bytes.
fn read_bounded(
    reader: &mut impl Read,
    declared_size: u64,
    limit: u64,
    path: &str,
) -> Result<Vec<u8>, RasterlottieError> {
    if declared_size > limit || declared_size > isize::MAX as u64 {
        return Err(admission_error(
            path,
            "declared expanded size exceeds remaining budget",
        ));
    }
    let mut bytes = Vec::new();
    if declared_size > 0 {
        #[cfg(test)]
        ALLOCATION_REQUESTS.with(|count| count.set(count.get() + 1));
        bytes
            .try_reserve_exact(declared_size as usize)
            .map_err(|_| admission_error(path, "cannot reserve bounded entry buffer"))?;
    }
    let mut chunk = [0; 8192];
    loop {
        let available = limit.saturating_sub(bytes.len() as u64);
        let requested = usize::try_from(available.saturating_add(1))
            .unwrap_or(usize::MAX)
            .min(chunk.len());
        let count = reader.read(&mut chunk[..requested]).map_err(|error| {
            RasterlottieError::InvalidDotLottie {
                detail: format!("failed to read archive entry `{path}`: {error}"),
            }
        })?;
        if count == 0 {
            return Ok(bytes);
        }
        if count as u64 > available {
            return Err(admission_error(
                path,
                "actual expanded size exceeds remaining budget",
            ));
        }
        if count > bytes.capacity() - bytes.len() {
            #[cfg(test)]
            ALLOCATION_REQUESTS.with(|requests| requests.set(requests.get() + 1));
            bytes
                .try_reserve_exact(count)
                .map_err(|_| admission_error(path, "cannot reserve bounded entry buffer"))?;
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}

fn read_archive_bytes_optional(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    candidates: &[String],
    per_entry_limit: u64,
    remaining: &mut u64,
) -> Result<Option<(String, Vec<u8>)>, RasterlottieError> {
    for candidate in candidates {
        match archive.by_name_with_options(
            candidate,
            ZipReadOptions::new().enforce_header_consistency(true),
        ) {
            Ok(mut file) => {
                let declared_size = file.size();
                let bytes = read_bounded(
                    &mut file,
                    declared_size,
                    per_entry_limit.min(*remaining),
                    candidate,
                )?;
                *remaining = remaining.checked_sub(bytes.len() as u64).ok_or_else(|| {
                    admission_error(candidate, "aggregate expanded size overflow")
                })?;
                return Ok(Some((candidate.clone(), bytes)));
            }
            Err(ZipError::FileNotFound) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(None)
}

#[cfg(feature = "images")]
fn embed_archive_images(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    animation: &mut Animation,
    per_image_limit: u64,
    remaining: &mut u64,
) -> Result<(), RasterlottieError> {
    for asset in &mut animation.assets {
        if !asset.is_image_asset() || asset.is_embedded_image_asset() {
            continue;
        }

        let candidates = archive_image_candidates(asset);
        let Some((path, bytes)) =
            read_archive_bytes_optional(archive, &candidates, per_image_limit, remaining)?
        else {
            continue;
        };
        let media_type = media_type_for_path(&path);
        asset.base_path = None;
        asset.embedded = Some(1);
        let mut data_url = format!("data:{media_type};base64,");
        let encoded_len = base64::encoded_len(bytes.len(), true)
            .ok_or_else(|| admission_error(&path, "base64 size overflow"))?;
        data_url
            .try_reserve_exact(encoded_len)
            .map_err(|_| admission_error(&path, "cannot reserve bounded base64 buffer"))?;
        general_purpose::STANDARD.encode_string(bytes, &mut data_url);
        asset.path = Some(data_url);
    }

    Ok(())
}

#[cfg(feature = "images")]
fn archive_image_candidates(asset: &Asset) -> Vec<String> {
    let mut candidates = Vec::new();
    let Some(path) = asset.path.as_deref() else {
        return candidates;
    };

    let relative_path = normalize_archive_path(path);
    if relative_path.is_empty() {
        return candidates;
    }

    if let Some(base_path) = asset.base_path.as_deref() {
        let normalized_base = normalize_archive_path(base_path);
        if !normalized_base.is_empty() {
            push_candidate(
                &mut candidates,
                join_archive_path(&normalized_base, &relative_path),
            );
        }
    }

    push_candidate(&mut candidates, relative_path.clone());
    push_candidate(&mut candidates, join_archive_path("i", &relative_path));
    push_candidate(&mut candidates, join_archive_path("images", &relative_path));

    if let Some(file_name) = relative_path.rsplit('/').next()
        && file_name != relative_path
    {
        push_candidate(&mut candidates, join_archive_path("i", file_name));
        push_candidate(&mut candidates, join_archive_path("images", file_name));
    }

    candidates
}

#[cfg(feature = "images")]
fn media_type_for_path(path: &str) -> &'static str {
    let extension = path.rsplit('.').next().unwrap_or_default();
    if extension.eq_ignore_ascii_case("png") {
        "image/png"
    } else if extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg") {
        "image/jpeg"
    } else if extension.eq_ignore_ascii_case("webp") {
        "image/webp"
    } else if extension.eq_ignore_ascii_case("gif") {
        "image/gif"
    } else if extension.eq_ignore_ascii_case("bmp") {
        "image/bmp"
    } else {
        "application/octet-stream"
    }
}

fn normalize_archive_path(path: &str) -> String {
    let mut normalized = path.replace('\\', "/");
    while let Some(stripped) = normalized.strip_prefix("./") {
        normalized = stripped.to_string();
    }
    normalized.trim_start_matches('/').to_string()
}

fn join_archive_path(base: &str, path: &str) -> String {
    let base = base.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    if base.is_empty() {
        path.to_string()
    } else if path.is_empty() {
        base.to_string()
    } else {
        format!("{base}/{path}")
    }
}

fn path_uses_json_extension(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
}

fn push_candidate(candidates: &mut Vec<String>, candidate: String) {
    if !candidate.is_empty() && !candidates.iter().any(|existing| existing == &candidate) {
        candidates.push(candidate);
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read, Write};

    #[cfg(feature = "images")]
    use image::{ColorType, ImageEncoder, Rgba, RgbaImage, codecs::png::PngEncoder};
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    use super::{ALLOCATION_REQUESTS, load_animation_from_dotlottie_bytes, read_bounded};

    #[test]
    fn bounded_reader_rejects_headers_without_reading_and_limits_actual_output() {
        struct CountedReader {
            remaining: usize,
            read: usize,
        }
        impl Read for CountedReader {
            fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
                let count = output.len().min(self.remaining);
                output[..count].fill(b'x');
                self.remaining -= count;
                self.read += count;
                Ok(count)
            }
        }
        let mut reader = CountedReader {
            remaining: 100_000,
            read: 0,
        };
        ALLOCATION_REQUESTS.with(|count| count.set(0));
        assert!(read_bounded(&mut reader, 17, 16, "large").is_err());
        assert_eq!(reader.read, 0);
        assert_eq!(ALLOCATION_REQUESTS.with(std::cell::Cell::get), 0);
        assert!(read_bounded(&mut reader, 0, 16, "lying-header").is_err());
        assert_eq!(reader.read, 17);
        let mut exact = Cursor::new([b'x'; 16]);
        assert_eq!(
            read_bounded(&mut exact, 16, 16, "exact").unwrap(),
            [b'x'; 16]
        );
        let mut zero = CountedReader {
            remaining: 1,
            read: 0,
        };
        assert!(read_bounded(&mut zero, 0, 0, "zero").is_err());
        assert_eq!(zero.read, 1);
        let mut huge = CountedReader {
            remaining: 1,
            read: 0,
        };
        assert!(read_bounded(&mut huge, u64::MAX, u64::MAX, "huge").is_err());
        assert_eq!(huge.read, 0);
        let mut large = CountedReader {
            remaining: 32 * 1024,
            read: 0,
        };
        ALLOCATION_REQUESTS.with(|count| count.set(0));
        assert_eq!(
            read_bounded(&mut large, 32 * 1024, 32 * 1024, "multi-chunk")
                .unwrap()
                .len(),
            32 * 1024
        );
        assert_eq!(ALLOCATION_REQUESTS.with(std::cell::Cell::get), 1);
    }

    fn archive_from_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (path, bytes) in entries {
            writer.start_file(*path, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn dotlottie_loader_prefers_manifest_initial_animation() {
        let archive = archive_from_entries(&[
            (
                "manifest.json",
                br#"{
                    "version":"2",
                    "animations":[{"id":"first"},{"id":"second"}],
                    "initial":{"animation":"second"}
                }"#,
            ),
            (
                "a/first.json",
                br#"{"v":"5.7.6","fr":30,"ip":0,"op":10,"w":16,"h":16,"layers":[{"ty":4,"nm":"First","ind":1,"shapes":[]}]}"#,
            ),
            (
                "a/second.json",
                br#"{"v":"5.7.6","fr":30,"ip":0,"op":10,"w":16,"h":16,"layers":[{"ty":4,"nm":"Second","ind":1,"shapes":[]}]}"#,
            ),
        ]);

        let animation = load_animation_from_dotlottie_bytes(&archive).unwrap();
        assert_eq!(animation.layers[0].name, "Second");
    }

    #[test]
    fn dotlottie_loader_falls_back_to_active_animation_id() {
        let archive = archive_from_entries(&[
            (
                "manifest.json",
                br#"{
                    "version":"1",
                    "activeAnimationId":"hero",
                    "animations":[{"id":"hero"}]
                }"#,
            ),
            (
                "animations/hero.json",
                br#"{"v":"5.7.6","fr":30,"ip":0,"op":10,"w":16,"h":16,"layers":[{"ty":4,"nm":"Hero","ind":1,"shapes":[]}]}"#,
            ),
        ]);

        let animation = load_animation_from_dotlottie_bytes(&archive).unwrap();
        assert_eq!(animation.layers[0].name, "Hero");
    }

    #[cfg(feature = "images")]
    #[test]
    fn dotlottie_loader_embeds_archive_images_into_data_urls() {
        let archive = archive_from_entries(&[
            ("manifest.json", br#"{"animations":[{"id":"hero"}]}"#),
            (
                "animations/hero.json",
                br#"{
                    "v":"5.7.6",
                    "fr":30,
                    "ip":0,
                    "op":10,
                    "w":16,
                    "h":16,
                    "assets":[{"id":"img","w":1,"h":1,"u":"images/","p":"cat.png"}],
                    "layers":[]
                }"#,
            ),
            ("images/cat.png", &solid_png_bytes()),
        ]);

        let animation = load_animation_from_dotlottie_bytes(&archive).unwrap();
        let asset = &animation.assets[0];
        assert_eq!(asset.base_path, None);
        assert_eq!(asset.embedded, Some(1));
        assert!(
            asset
                .path
                .as_deref()
                .is_some_and(|path| path.starts_with("data:image/png;base64,"))
        );
    }

    #[cfg(feature = "images")]
    fn solid_png_bytes() -> Vec<u8> {
        let image = RgbaImage::from_pixel(1, 1, Rgba([255, 0, 0, 255]));
        let mut bytes = Vec::new();
        PngEncoder::new(&mut bytes)
            .write_image(image.as_raw(), 1, 1, ColorType::Rgba8.into())
            .unwrap();
        bytes
    }
}

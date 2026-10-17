use std::{
    io::{Cursor, Read},
    path::Path,
};

#[cfg(feature = "images")]
use base64::{Engine, engine::general_purpose};
use serde::Deserialize;
use zip::{ZipArchive, result::ZipError};

#[cfg(feature = "images")]
use crate::Asset;
use crate::{Animation, RasterlottieError};

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

pub fn load_animation_from_dotlottie_bytes(
    dotlottie: &[u8],
) -> Result<Animation, RasterlottieError> {
    let mut archive = ZipArchive::new(Cursor::new(dotlottie))?;
    let manifest_json = read_archive_string(&mut archive, "manifest.json")?;
    let manifest = serde_json::from_str::<DotLottieManifest>(&manifest_json).map_err(|error| {
        RasterlottieError::InvalidDotLottie {
            detail: format!("failed to parse manifest.json: {error}"),
        }
    })?;
    let animation_id = select_animation_id(&manifest)?;
    let animation_json =
        read_archive_string_any(&mut archive, &animation_candidate_paths(animation_id))?;
    #[cfg(feature = "images")]
    let mut animation = Animation::from_json_str(&animation_json)?;
    #[cfg(not(feature = "images"))]
    let animation = Animation::from_json_str(&animation_json)?;
    #[cfg(feature = "images")]
    embed_archive_images(&mut archive, &mut animation)?;
    Ok(animation)
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

fn read_archive_string(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    path: &str,
) -> Result<String, RasterlottieError> {
    let normalized = normalize_archive_path(path);
    read_archive_string_any(archive, &[normalized])
}

fn read_archive_string_any(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    candidates: &[String],
) -> Result<String, RasterlottieError> {
    let (path, bytes) = read_archive_bytes_any(archive, candidates)?;
    String::from_utf8(bytes).map_err(|error| RasterlottieError::InvalidDotLottie {
        detail: format!("archive entry `{path}` is not valid UTF-8: {error}"),
    })
}

fn read_archive_bytes_any(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    candidates: &[String],
) -> Result<(String, Vec<u8>), RasterlottieError> {
    read_archive_bytes_optional(archive, candidates)?.ok_or_else(|| {
        RasterlottieError::InvalidDotLottie {
            detail: format!(
                "archive is missing required entry; tried {}",
                candidates.join(", ")
            ),
        }
    })
}

fn read_archive_bytes_optional(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    candidates: &[String],
) -> Result<Option<(String, Vec<u8>)>, RasterlottieError> {
    for candidate in candidates {
        match archive.by_name(candidate) {
            Ok(mut file) => {
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes).map_err(|error| {
                    RasterlottieError::InvalidDotLottie {
                        detail: format!("failed to read archive entry `{candidate}`: {error}"),
                    }
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
) -> Result<(), RasterlottieError> {
    for asset in &mut animation.assets {
        if !asset.is_image_asset() || asset.is_embedded_image_asset() {
            continue;
        }

        let candidates = archive_image_candidates(asset);
        let Some((path, bytes)) = read_archive_bytes_optional(archive, &candidates)? else {
            continue;
        };
        let media_type = media_type_for_path(&path);
        asset.base_path = None;
        asset.embedded = Some(1);
        asset.path = Some(format!(
            "data:{media_type};base64,{}",
            general_purpose::STANDARD.encode(bytes)
        ));
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
    use std::io::{Cursor, Write};

    #[cfg(feature = "images")]
    use image::{ColorType, ImageEncoder, Rgba, RgbaImage, codecs::png::PngEncoder};
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    use super::load_animation_from_dotlottie_bytes;

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

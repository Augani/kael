use crate::{
    AtlasAllocationClass, AtlasKey, AtlasTextureKind, DevicePixels, FontId, GlyphId,
    GlyphRasterMode, ImageId, Point, RenderGlyphParams, RenderImageParams,
    SMALL_IMAGE_ATLAS_PAGE_SIZE, Size, px,
};

fn image_key() -> AtlasKey {
    AtlasKey::Image(RenderImageParams {
        image_id: ImageId(42),
        frame_index: 0,
    })
}

fn glyph_key(raster_mode: GlyphRasterMode, is_emoji: bool) -> AtlasKey {
    AtlasKey::Glyph(RenderGlyphParams {
        font_id: FontId(7),
        glyph_id: GlyphId(11),
        font_size: px(12.),
        subpixel_variant: Point { x: 0, y: 0 },
        scale_factor: 1.0,
        is_emoji,
        raster_mode,
    })
}

fn shadow_key() -> AtlasKey {
    AtlasKey::Shadow(crate::shadow_cache::ShadowAtlasParams::new(
        crate::size(crate::DevicePixels(24), crate::DevicePixels(18)),
        crate::Corners {
            top_left: crate::ScaledPixels(4.0),
            top_right: crate::ScaledPixels(4.0),
            bottom_right: crate::ScaledPixels(4.0),
            bottom_left: crate::ScaledPixels(4.0),
        },
        crate::ScaledPixels(3.0),
        crate::hsla(0.0, 0.0, 0.0, 0.25),
        false,
    ))
}

#[test]
fn small_images_use_shared_thumbnail_pages() {
    let key = image_key();
    let size = Size {
        width: DevicePixels(64),
        height: DevicePixels(64),
    };

    assert_eq!(
        key.allocation_class(size),
        AtlasAllocationClass::SharedSmallImage
    );
    assert_eq!(
        AtlasAllocationClass::SharedSmallImage.texture_size(
            size,
            Size {
                width: DevicePixels(1024),
                height: DevicePixels(1024),
            },
            Size {
                width: DevicePixels(16384),
                height: DevicePixels(16384),
            },
        ),
        SMALL_IMAGE_ATLAS_PAGE_SIZE,
    );
}

#[test]
fn large_images_use_dedicated_textures() {
    let key = image_key();
    let size = Size {
        width: DevicePixels(512),
        height: DevicePixels(320),
    };

    assert_eq!(
        key.allocation_class(size),
        AtlasAllocationClass::DedicatedLargeImage
    );
    assert_eq!(
        AtlasAllocationClass::DedicatedLargeImage.texture_size(
            size,
            Size {
                width: DevicePixels(1024),
                height: DevicePixels(1024),
            },
            Size {
                width: DevicePixels(16384),
                height: DevicePixels(16384),
            },
        ),
        size,
    );
}

#[test]
fn grayscale_glyphs_use_monochrome_atlas() {
    assert_eq!(
        glyph_key(GlyphRasterMode::Grayscale, false).texture_kind(),
        AtlasTextureKind::Monochrome
    );
}

#[test]
fn subpixel_and_emoji_glyphs_use_polychrome_atlas() {
    assert_eq!(
        glyph_key(GlyphRasterMode::Subpixel, false).texture_kind(),
        AtlasTextureKind::Polychrome
    );
    assert_eq!(
        glyph_key(GlyphRasterMode::Grayscale, true).texture_kind(),
        AtlasTextureKind::Polychrome
    );
}

#[test]
fn shadow_tiles_use_polychrome_atlas() {
    assert_eq!(shadow_key().texture_kind(), AtlasTextureKind::Polychrome);
}

#[test]
fn atlas_eviction_is_a_noop_within_budget() {
    let tiles = [(1u64, 100u64), (2, 100)];
    assert!(crate::select_atlas_evictions(&tiles, 200, 256, 5).is_empty());
}

#[test]
fn atlas_eviction_sheds_least_recently_used_until_within_budget() {
    // Four 100-byte tiles, 400 total, budget 250 -> must free >= 150 -> evict 2 oldest.
    let tiles = [(4u64, 100u64), (1, 100), (3, 100), (2, 100)];
    let mut victims = crate::select_atlas_evictions(&tiles, 400, 250, 10);
    victims.sort_unstable();
    // Indices 1 (frame 1) and 3 (frame 2) are the two oldest.
    assert_eq!(victims, vec![1, 3]);
}

#[test]
fn atlas_eviction_never_evicts_tiles_used_this_frame() {
    // Over budget, but every tile was used in the current frame (10) -> nothing is safe to
    // evict, so the atlas stays over budget rather than risk corrupting the in-flight render.
    let tiles = [(10u64, 100u64), (10, 100), (10, 100)];
    assert!(crate::select_atlas_evictions(&tiles, 300, 100, 10).is_empty());

    // One older tile present -> only that one is evictable.
    let mixed = [(10u64, 100u64), (9, 100), (10, 100)];
    assert_eq!(crate::select_atlas_evictions(&mixed, 300, 100, 10), vec![1]);
}

#[test]
fn atlas_eviction_frees_what_it_can_when_evictable_tiles_are_insufficient() {
    // Need to free 250 but only one 100-byte tile is old enough -> evict it and accept the
    // atlas remains over budget (correctness over budget adherence).
    let tiles = [(10u64, 100u64), (1, 100), (10, 100)];
    assert_eq!(crate::select_atlas_evictions(&tiles, 300, 50, 10), vec![1]);
}

#[test]
fn atlas_payload_validation_rejects_invalid_dimensions_and_byte_lengths() {
    let valid = Size {
        width: DevicePixels(4),
        height: DevicePixels(3),
    };
    assert_eq!(
        crate::validate_atlas_payload(valid, AtlasTextureKind::Monochrome, 12).unwrap(),
        12
    );
    assert_eq!(
        crate::validate_atlas_payload(valid, AtlasTextureKind::Polychrome, 48).unwrap(),
        48
    );
    assert!(crate::validate_atlas_payload(valid, AtlasTextureKind::Polychrome, 47).is_err());
    assert!(
        crate::validate_atlas_payload(
            Size {
                width: DevicePixels(0),
                height: DevicePixels(3),
            },
            AtlasTextureKind::Monochrome,
            0,
        )
        .is_err()
    );
    assert!(
        crate::validate_atlas_payload(
            Size {
                width: DevicePixels(crate::MAX_ATLAS_TEXTURE_DIMENSION + 1),
                height: DevicePixels(1),
            },
            AtlasTextureKind::Monochrome,
            1,
        )
        .is_err()
    );
}

#[test]
fn gpu_dimensions_are_finite_nonzero_and_capped() {
    assert_eq!(super::super::safe_gpu_dimension(f32::NAN), 1);
    assert_eq!(super::super::safe_gpu_dimension(f32::INFINITY), 1);
    assert_eq!(super::super::safe_gpu_dimension(-10.0), 1);
    assert_eq!(super::super::safe_gpu_dimension(0.0), 1);
    assert_eq!(super::super::safe_gpu_dimension(42.9), 42);
    assert_eq!(super::super::safe_gpu_dimension(100_000.0), 16_384);
}

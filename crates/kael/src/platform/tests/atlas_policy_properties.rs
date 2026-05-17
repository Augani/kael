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

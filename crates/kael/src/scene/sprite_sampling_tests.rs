//! Native fixture for filtering isolation between independently owned atlas tiles.
use crate::{
    AtlasKey, AtlasTile, Background, Bounds, ColorFilter, ContentMask, Corners, DevicePixels, Hsla,
    ImageId, MonochromeSprite, PlatformAtlas, PolychromeSprite, Quad, RenderImageParams,
    RenderSvgParams, ScaledPixels, Scene, TransformationMatrix, hsla, point, size,
};
use std::borrow::Cow;

fn tile(atlas: &dyn PlatformAtlas, key: AtlasKey, side: i32, bytes: Vec<u8>) -> AtlasTile {
    atlas
        .get_or_insert_with_size(
            &key,
            size(DevicePixels(side), DevicePixels(side)),
            &mut || {
                Ok(Some((
                    size(DevicePixels(side), DevicePixels(side)),
                    Cow::Borrowed(&bytes),
                )))
            },
        )
        .unwrap()
        .unwrap()
}

pub(crate) fn packed_sprite_scene(atlas: &dyn PlatformAtlas) -> Scene {
    let svg = |path: &str| {
        AtlasKey::Svg(RenderSvgParams {
            path: path.to_owned().into(),
            size: size(DevicePixels(8), DevicePixels(8)),
        })
    };
    let image = |id| {
        AtlasKey::Image(RenderImageParams {
            image_id: ImageId(id),
            frame_index: 0,
        })
    };
    let mono = tile(atlas, svg("sampler-owned"), 8, vec![255; 64]);
    let mono_neighbor = tile(atlas, svg("sampler-neighbor"), 8, vec![0; 64]);
    let poly = tile(atlas, image(991), 8, [0, 0, 255, 255].repeat(64));
    let poly_neighbor = tile(atlas, image(992), 8, [255, 0, 0, 255].repeat(64));
    for (owned, neighbor) in [(&mono, &mono_neighbor), (&poly, &poly_neighbor)] {
        assert_eq!(
            owned.texture_id, neighbor.texture_id,
            "fixture must use the same packed page"
        );
        assert_eq!(owned.bounds.origin.y, neighbor.bounds.origin.y);
        assert_eq!(
            owned.bounds.right(),
            neighbor.bounds.origin.x,
            "fixture must expose an adjacent texel"
        );
    }
    // This two-color image verifies interior bilinear interpolation remains
    // unchanged. Shrinking the vertex UV range instead of clamping samples
    // would shift its interior gradient.
    let gradient = tile(
        atlas,
        image(993),
        2,
        [0, 0, 255, 255, 255, 0, 0, 255].repeat(2),
    );
    let viewport = Bounds::new(
        point(ScaledPixels(0.0), ScaledPixels(0.0)),
        size(ScaledPixels(32.0), ScaledPixels(40.0)),
    );
    let bounds = |x, y, side| {
        Bounds::new(
            point(ScaledPixels(x), ScaledPixels(y)),
            size(ScaledPixels(side), ScaledPixels(side)),
        )
    };
    let transform = TransformationMatrix {
        translation: [0.2, 0.2],
        ..TransformationMatrix::unit()
    };
    let mut scene = Scene::default();
    scene.insert_primitive(Quad {
        bounds: viewport,
        content_mask: ContentMask { bounds: viewport },
        background: Background::from(hsla(0.0, 0.0, 0.0, 1.0)),
        ..Default::default()
    });
    scene.insert_primitive(MonochromeSprite {
        order: 0,
        pad: 0,
        bounds: bounds(0.0, 0.0, 16.0),
        content_mask: ContentMask { bounds: viewport },
        color: hsla(0.0, 1.0, 0.5, 1.0),
        tile: mono,
        transformation: transform,
        rounded_clip_bounds: Bounds::default(),
        rounded_clip_radii: Corners::default(),
        color_filter: ColorFilter::identity(),
    });
    for (tile, bounds) in [
        (poly, bounds(0.0, 18.0, 16.0)),
        (gradient, bounds(18.0, 18.0, 8.0)),
    ] {
        scene.insert_primitive(PolychromeSprite {
            order: 0,
            pad: 0,
            grayscale: false,
            opacity: 1.0,
            bounds,
            content_mask: ContentMask { bounds: viewport },
            corner_radii: Corners::default(),
            tile,
            sprite_kind: crate::POLYCHROME_SPRITE_KIND_COLOR,
            color: Hsla::transparent_black(),
            pad3: 0,
            rounded_clip_bounds: Bounds::default(),
            rounded_clip_radii: Corners::default(),
            color_filter: ColorFilter::identity(),
            transformation: transform,
            blur_radius: 0.0,
            pad2: 0,
        });
    }
    scene.finish();
    scene
}

pub(crate) fn assert_packed_sprite_pixels(bgra: &[u8]) {
    for (x, y, expected) in [
        (15, 8, [0, 0, 255, 255]),
        (15, 26, [0, 0, 255, 255]),
        (21, 21, [83, 0, 172, 255]),
    ] {
        let pixel = &bgra[(y * 32 + x) * 4..][..4];
        eprintln!("packed atlas sprite pixel ({x},{y}) BGRA={pixel:?}");
        for (actual, expected) in pixel.iter().zip(expected) {
            assert!(
                actual.abs_diff(expected) <= 2,
                "pixel ({x},{y}) is {pixel:?}, expected channel {expected}"
            );
        }
    }
}

/// Remove the fixture's cache owners; retained scenes keep the retired regions
/// protected through the atlas's successful-frame guard.
pub(crate) fn remove_packed_sprite_keys(atlas: &dyn PlatformAtlas) {
    for path in ["sampler-owned", "sampler-neighbor"] {
        atlas.remove(&AtlasKey::Svg(RenderSvgParams {
            path: path.to_owned().into(),
            size: size(DevicePixels(8), DevicePixels(8)),
        }));
    }
    for id in [991, 992, 993] {
        atlas.remove(&AtlasKey::Image(RenderImageParams {
            image_id: ImageId(id),
            frame_index: 0,
        }));
    }
}

pub(crate) fn reject_packed_sprite_growth_before_raster(atlas: &dyn PlatformAtlas) {
    atlas.set_hard_admission_limits(crate::AtlasAdmissionLimits {
        max_bytes: 0,
        max_tiles: 0,
        max_pages: 0,
    });
    let key = AtlasKey::Image(RenderImageParams {
        image_id: ImageId(999),
        frame_index: 0,
    });
    assert!(
        atlas
            .get_or_insert_with_size(
                &key,
                size(DevicePixels(8), DevicePixels(8)),
                &mut || panic!("live pages must reject growth before raster")
            )
            .is_err()
    );
}

pub(crate) fn surviving_page_key(id: usize) -> AtlasKey {
    AtlasKey::CachedSurface(crate::CachedSurfaceParams {
        cache_id: id as u64,
        size: size(DevicePixels(1024), DevicePixels(512)),
    })
}

pub(crate) fn surviving_page_tile(
    atlas: &dyn PlatformAtlas,
    id: usize,
    color: [u8; 4],
) -> AtlasTile {
    atlas
        .get_or_insert_with_size(
            &surviving_page_key(id),
            size(DevicePixels(1024), DevicePixels(512)),
            &mut || {
                Ok(Some((
                    size(DevicePixels(1024), DevicePixels(512)),
                    Cow::Owned(color.repeat(1024 * 512)),
                )))
            },
        )
        .unwrap()
        .unwrap()
}

pub(crate) fn surviving_page_scene(tile: AtlasTile) -> Scene {
    let bounds = Bounds::new(
        point(ScaledPixels(0.0), ScaledPixels(0.0)),
        size(ScaledPixels(16.0), ScaledPixels(16.0)),
    );
    let mut scene = Scene::default();
    scene.insert_primitive(PolychromeSprite {
        order: 0,
        pad: 0,
        grayscale: false,
        opacity: 1.0,
        bounds,
        content_mask: ContentMask { bounds },
        corner_radii: Corners::default(),
        tile,
        sprite_kind: crate::POLYCHROME_SPRITE_KIND_COLOR,
        color: Hsla::transparent_black(),
        pad3: 0,
        pad2: 0,
        blur_radius: 0.0,
        rounded_clip_bounds: Bounds::default(),
        rounded_clip_radii: Corners::default(),
        transformation: TransformationMatrix::unit(),
        color_filter: ColorFilter::identity(),
    });
    scene.finish();
    scene
}

use super::*;
use crate::{
    AtlasAdmissionLimits, ContentMask, Corners, PlatformAtlas, RenderIconAtlasParams, hsla,
};
use wasm_bindgen_test::wasm_bindgen_test;

fn pixels(renderer: &WebGlSceneRenderer) -> Vec<u8> {
    let mut pixels = vec![0; 16 * 16 * 4];
    renderer
        .gl
        .read_pixels_with_opt_u8_array(0, 0, 16, 16, Gl::RGBA, Gl::UNSIGNED_BYTE, Some(&mut pixels))
        .unwrap();
    assert_eq!(renderer.gl.get_error(), Gl::NO_ERROR);
    pixels
}

#[wasm_bindgen_test]
fn browser_atlas_gpu_mirror_replay_pressure_retirement_and_reupload_preserve_pixels() {
    let canvas: HtmlCanvasElement = web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .create_element("canvas")
        .unwrap()
        .unchecked_into();
    canvas.set_width(16);
    canvas.set_height(16);
    let mut renderer =
        WebGlSceneRenderer::new(&canvas, crate::size(DevicePixels(16), DevicePixels(16))).unwrap();
    let atlas = renderer.atlas();
    atlas.set_hard_admission_limits(AtlasAdmissionLimits {
        max_bytes: 512,
        max_tiles: 1,
        max_pages: 1,
    });
    let key = crate::AtlasKey::IconAtlas(RenderIconAtlasParams {
        edge: DevicePixels(16),
    });
    let tile_size = crate::size(DevicePixels(16), DevicePixels(16));
    let tile = atlas
        .get_or_insert_with_size(&key, tile_size, &mut || {
            Ok(Some((tile_size, std::borrow::Cow::Owned(vec![255; 256]))))
        })
        .unwrap()
        .unwrap();
    let viewport = Bounds::new(
        crate::point(ScaledPixels(0.0), ScaledPixels(0.0)),
        crate::size(ScaledPixels(16.0), ScaledPixels(16.0)),
    );
    let mut plain = Scene::default();
    plain.insert_primitive(Quad {
        bounds: viewport,
        content_mask: ContentMask { bounds: viewport },
        background: hsla(0.0, 0.0, 1.0, 1.0).into(),
        ..Default::default()
    });
    plain.finish();
    let mut scene = Scene::default();
    scene.insert_primitive(plain.quads[0].clone());
    scene.insert_primitive(crate::MonochromeSprite {
        order: 0,
        pad: 0,
        bounds: Bounds::new(
            crate::point(ScaledPixels(0.0), ScaledPixels(0.0)),
            crate::size(ScaledPixels(8.0), ScaledPixels(8.0)),
        ),
        content_mask: ContentMask { bounds: viewport },
        color: hsla(0.0, 1.0, 0.5, 1.0),
        tile,
        transformation: Default::default(),
        rounded_clip_bounds: Default::default(),
        rounded_clip_radii: Corners::default(),
        color_filter: Default::default(),
    });
    scene.finish();
    renderer.draw(&scene).unwrap();
    let baseline = pixels(&renderer);
    // ReadPixels has bottom-left origin; the red square occupies the top left.
    assert_eq!(
        &baseline[(13 * 16 + 2) * 4..(13 * 16 + 2) * 4 + 4],
        &[255, 0, 0, 255]
    );
    assert_eq!(
        &baseline[(2 * 16 + 13) * 4..(2 * 16 + 13) * 4 + 4],
        &[255; 4]
    );
    for _ in 0..8 {
        renderer.draw(&scene).unwrap();
        renderer.shed_atlas_memory();
        assert_eq!(pixels(&renderer), baseline);
    }
    atlas.remove(&key);
    for _ in 0..3 {
        renderer.draw(&plain).unwrap();
        assert_eq!(renderer.textures.len(), 1);
    }
    renderer.draw(&plain).unwrap();
    renderer.shed_atlas_memory();
    assert!(
        renderer.textures.is_empty(),
        "retired GPU mirror is actually deleted"
    );
    let restored = atlas
        .get_or_insert_with_size(&key, tile_size, &mut || {
            Ok(Some((tile_size, std::borrow::Cow::Owned(vec![255; 256]))))
        })
        .unwrap()
        .unwrap();
    scene.monochrome_sprites[0].tile = restored;
    scene.finish();
    renderer.draw(&scene).unwrap();
    assert_eq!(
        pixels(&renderer),
        baseline,
        "atlas reupload restores the same rendered pixels"
    );
    renderer.destroy();
}

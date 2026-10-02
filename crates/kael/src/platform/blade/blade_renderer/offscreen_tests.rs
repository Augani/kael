use super::*;
use crate::{ContentMask, TransformationMatrix, hsla, point, size};

fn renderer(alpha: gpu::AlphaMode) -> BladeRenderer {
    // Tests exercise a real device without an AppKit/X11 presentation surface.
    let context = BladeContext {
        gpu: Arc::new(
            unsafe {
                gpu::Context::init(gpu::ContextDesc {
                    presentation: false,
                    validation: false,
                    ..Default::default()
                })
            }
            .expect("required native Blade device"),
        ),
    };
    BladeRenderer::with_surface(
        &context,
        None,
        gpu::SurfaceInfo {
            format: gpu::TextureFormat::Bgra8Unorm,
            alpha,
        },
        gpu::SurfaceConfig {
            size: gpu::Extent {
                width: 16,
                height: 16,
                depth: 1,
            },
            usage: gpu::TextureUsage::TARGET,
            display_sync: gpu::DisplaySync::Recent,
            color_space: gpu::ColorSpace::Srgb,
            allow_exclusive_full_screen: false,
            transparent: true,
        },
    )
    .unwrap()
}

fn quad(side: f32, color: Hsla) -> Quad {
    let bounds = Bounds::new(
        point(ScaledPixels(0.0), ScaledPixels(0.0)),
        size(ScaledPixels(side), ScaledPixels(side)),
    );
    Quad {
        bounds,
        content_mask: ContentMask { bounds },
        background: color.into(),
        transform: TransformationMatrix::unit(),
        ..Default::default()
    }
}

fn path(side: f32, color: Hsla) -> Path<ScaledPixels> {
    let mut builder = crate::PathBuilder::fill();
    builder.move_to(point(crate::px(0.0), crate::px(0.0)));
    builder.line_to(point(crate::px(side), crate::px(0.0)));
    builder.line_to(point(crate::px(side), crate::px(side)));
    builder.line_to(point(crate::px(0.0), crate::px(side)));
    builder.close();
    let mut path = builder.build().unwrap();
    path.color = color.into();
    path.content_mask = ContentMask {
        bounds: path.bounds,
    };
    path.scale(1.0)
}

#[test]
fn blade_quad_and_resize_leave_optional_scratch_unallocated() {
    let mut renderer = renderer(gpu::AlphaMode::PreMultiplied);
    let mut scene = Scene::default();
    scene.insert_primitive(quad(16.0, hsla(0.0, 1.0, 0.5, 1.0)));
    scene.finish();
    let frame = renderer.render_scene_to_bgra(&scene).unwrap();
    assert_eq!(
        &frame.bgra[8 * 16 * 4 + 8 * 4..8 * 16 * 4 + 8 * 4 + 4],
        &[0, 0, 255, 255]
    );
    renderer.update_drawable_size(size(DevicePixels(32), DevicePixels(24)));
    assert_eq!(renderer.scratch_texture_count(), 0);
    renderer.destroy();
}

#[test]
fn blade_translucent_paths_preserve_source_over_alpha() {
    for alpha in [
        gpu::AlphaMode::PreMultiplied,
        gpu::AlphaMode::PostMultiplied,
    ] {
        let mut renderer = renderer(alpha);
        let mut scene = Scene::default();
        scene.insert_primitive(quad(16.0, hsla(0.0, 1.0, 0.5, 0.5)));
        scene.insert_primitive(path(16.0, hsla(2.0 / 3.0, 1.0, 0.5, 0.5)));
        scene.finish();
        let frame = renderer.render_scene_to_bgra(&scene).unwrap();
        let pixel = &frame.bgra[8 * 16 * 4 + 8 * 4..8 * 16 * 4 + 8 * 4 + 4];
        for (&actual, expected) in pixel.iter().zip([128u8, 0, 64, 191]) {
            assert!(actual.abs_diff(expected) <= 2, "{alpha:?}: {pixel:?}");
        }
        renderer.destroy();
    }
}

fn blur_rect(bounds: Bounds<ScaledPixels>, sigma: f32, tint: Hsla) -> BlurRect {
    BlurRect {
        order: 0,
        bounds,
        content_mask: ContentMask { bounds },
        blur_radius: ScaledPixels(sigma),
        tint,
        saturation: 1.0,
        corner_radii: Corners::default(),
        rounded_clip_bounds: Bounds::default(),
        rounded_clip_radii: Corners::default(),
    }
}

#[test]
fn blade_backdrop_fractional_capture_includes_last_visible_texel() {
    for alpha in [
        gpu::AlphaMode::PreMultiplied,
        gpu::AlphaMode::PostMultiplied,
    ] {
        let mut renderer = renderer(alpha);
        renderer.update_drawable_size(size(DevicePixels(64), DevicePixels(32)));
        let mut scene = Scene::default();
        scene.insert_primitive(quad(64.0, hsla(0.0, 1.0, 0.5, 1.0)));
        let mut blue = quad(32.0, hsla(2.0 / 3.0, 1.0, 0.5, 1.0));
        blue.bounds.origin.x = ScaledPixels(32.0);
        blue.bounds.size.width = ScaledPixels(1.0);
        blue.content_mask.bounds = blue.bounds;
        scene.insert_primitive(blue);
        scene.insert_primitive(blur_rect(
            Bounds::new(
                point(ScaledPixels(12.8), ScaledPixels(4.2)),
                size(ScaledPixels(20.0), ScaledPixels(20.0)),
            ),
            0.0,
            Hsla::transparent_black(),
        ));
        scene.finish();
        let frame = renderer.render_scene_to_bgra(&scene).unwrap();
        assert_eq!(
            &frame.bgra[((16 * 64) + 32) * 4..][..4],
            &[255, 0, 0, 255],
            "{alpha:?}"
        );
        renderer.destroy();
    }
}

#[test]
fn blade_backdrop_respects_own_and_ancestor_rounded_clips() {
    for alpha in [
        gpu::AlphaMode::PreMultiplied,
        gpu::AlphaMode::PostMultiplied,
    ] {
        let mut renderer = renderer(alpha);
        renderer.update_drawable_size(size(DevicePixels(32), DevicePixels(32)));
        for ancestor in [false, true] {
            let mut scene = Scene::default();
            scene.insert_primitive(quad(32.0, hsla(2.0 / 3.0, 1.0, 0.5, 1.0)));
            let bounds = Bounds::new(
                point(ScaledPixels(0.0), ScaledPixels(0.0)),
                size(ScaledPixels(32.0), ScaledPixels(32.0)),
            );
            let mut panel = blur_rect(bounds, 1.0, hsla(0.0, 1.0, 0.5, 1.0));
            panel.corner_radii = Corners::all(ScaledPixels(8.0));
            if ancestor {
                panel.rounded_clip_bounds = Bounds::new(
                    point(ScaledPixels(8.0), ScaledPixels(8.0)),
                    size(ScaledPixels(16.0), ScaledPixels(16.0)),
                );
                panel.rounded_clip_radii = Corners::all(ScaledPixels(8.0));
                panel.content_mask.bounds = Bounds::new(
                    point(ScaledPixels(9.0), ScaledPixels(8.0)),
                    size(ScaledPixels(15.0), ScaledPixels(16.0)),
                );
            }
            scene.insert_primitive(panel);
            scene.finish();
            let frame = renderer.render_scene_to_bgra(&scene).unwrap();
            assert_eq!(&frame.bgra[((16 * 32) + 16) * 4..][..4], &[0, 0, 255, 255]);
            let outside = if ancestor { (9, 8) } else { (0, 0) };
            assert_eq!(
                &frame.bgra[((outside.1 * 32) + outside.0) * 4..][..4],
                &[255, 0, 0, 255],
                "{alpha:?}, ancestor={ancestor}"
            );
        }
        renderer.destroy();
    }
}

#[test]
fn blade_packed_sprite_filtering_isolates_neighbor_texels_and_preserves_interpolation() {
    for alpha in [
        gpu::AlphaMode::PreMultiplied,
        gpu::AlphaMode::PostMultiplied,
    ] {
        let mut renderer = renderer(alpha);
        renderer.update_drawable_size(size(DevicePixels(32), DevicePixels(40)));
        let scene = crate::scene::sprite_sampling_tests::packed_sprite_scene(&*renderer.atlas);
        let frame = renderer.render_scene_to_bgra(&scene).unwrap();
        crate::scene::sprite_sampling_tests::assert_packed_sprite_pixels(&frame.bgra);
        renderer.destroy();
    }
}

#[cfg(feature = "custom-shaders")]
#[test]
fn blade_scratch_pressure_recovery_preserves_owned_gpu_resources_and_pixels() {
    use crate::{CachedSurfaceParams, CachedSurfaceSnapshot, MemoryPressureLevel, PlatformAtlas};
    use std::borrow::Cow;
    let mut renderer = renderer(gpu::AlphaMode::PreMultiplied);
    let target = renderer
        .create_render_target(crate::RenderTargetDescriptor::rgba8(2, 2))
        .unwrap();
    let target_pixels = [17, 23, 31, 255].repeat(4);
    renderer
        .write_render_target(&target, &target_pixels)
        .unwrap();
    let buffer = renderer
        .create_gpu_buffer(crate::GpuBufferDescriptor { byte_len: 16 })
        .unwrap();
    let buffer_bytes = [1u32, 2, 3, 4]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    renderer
        .write_gpu_buffer(&buffer, 0, &buffer_bytes)
        .unwrap();
    let viewport = size(DevicePixels(16), DevicePixels(16));
    let tile = renderer
        .atlas
        .get_or_insert_with(
            &CachedSurfaceParams {
                cache_id: 1,
                size: viewport,
            }
            .into(),
            &mut || Ok(Some((viewport, Cow::Owned(vec![0; 16 * 16 * 4])))),
        )
        .unwrap()
        .unwrap();
    let mut scene = Scene::default();
    scene.insert_primitive(path(16.0, hsla(0.0, 1.0, 0.5, 1.0)));
    let bounds = Bounds::new(
        point(ScaledPixels(4.0), ScaledPixels(4.0)),
        size(ScaledPixels(8.0), ScaledPixels(8.0)),
    );
    scene.insert_primitive(BlurRect {
        bounds,
        content_mask: ContentMask { bounds },
        blur_radius: ScaledPixels(1.0),
        tint: hsla(2.0 / 3.0, 1.0, 0.5, 0.25),
        saturation: 1.0,
        corner_radii: Corners::default(),
        rounded_clip_bounds: Bounds::default(),
        rounded_clip_radii: Corners::default(),
        order: 0,
    });
    scene.request_cached_surface_snapshot(CachedSurfaceSnapshot {
        paint_operations: 0..scene.paint_operations.len(),
        source_bounds: Bounds::new(point(DevicePixels(0), DevicePixels(0)), viewport),
        target: tile,
    });
    scene.finish();
    let initial = renderer.render_scene_to_bgra(&scene).unwrap();
    assert_eq!(
        renderer.scratch_texture_count(),
        4 + usize::from(renderer.path_intermediate_msaa_texture.is_some())
    );
    let center = &initial.bgra[(8 * 16 + 8) * 4..(8 * 16 + 8) * 4 + 4];
    for (&actual, expected) in center.iter().zip([64u8, 0, 191, 255]) {
        assert!(actual.abs_diff(expected) <= 2, "blur center: {center:?}");
    }
    let path_texture = renderer.path_intermediate_texture;
    renderer.render_scene_to_bgra(&scene).unwrap();
    assert_eq!(
        renderer.path_intermediate_texture, path_texture,
        "same viewport reuses path scratch"
    );
    renderer.shed_memory(MemoryPressureLevel::Critical);
    assert_eq!(renderer.scratch_texture_count(), 0);
    assert!(target.is_valid() && buffer.is_valid());
    assert_eq!(
        renderer.read_render_target(&target).unwrap().pixels,
        target_pixels
    );
    assert_eq!(renderer.read_gpu_buffer(&buffer).unwrap(), buffer_bytes);
    assert_eq!(
        renderer.render_scene_to_bgra(&scene).unwrap().bgra,
        initial.bgra
    );
    renderer.update_drawable_size(size(DevicePixels(0), DevicePixels(0)));
    assert_eq!(renderer.scratch_texture_count(), 0);
    assert!(renderer.render_scene_to_bgra(&scene).is_err());
    renderer.draw(&scene);
    assert_eq!(renderer.scratch_texture_count(), 0);
    renderer.update_drawable_size(viewport);
    assert_eq!(
        renderer.render_scene_to_bgra(&scene).unwrap().bgra,
        initial.bgra
    );
    renderer.update_drawable_size(size(DevicePixels(24), DevicePixels(32)));
    assert_eq!(renderer.scratch_texture_count(), 0);
    renderer.render_scene_to_bgra(&scene).unwrap();
    renderer.destroy();
    assert_eq!(renderer.scratch_texture_count(), 0);
}

#[cfg(target_os = "macos")]
#[test]
fn blade_scratch_probe_measures_actual_device_allocation_at_4k() {
    let mut renderer = renderer(gpu::AlphaMode::PreMultiplied);
    renderer.update_drawable_size(size(DevicePixels(3840), DevicePixels(2160)));
    assert_eq!(renderer.scratch_texture_count(), 0);
    let before = renderer.gpu_allocated_bytes();
    renderer.ensure_path_intermediate();
    renderer.ensure_cached_surface();
    renderer.ensure_blur_intermediates();
    let allocated = renderer.gpu_allocated_bytes() - before;
    let minimum = 3840u64
        * 2160
        * 4
        * (4 + if renderer.rendering_parameters.path_sample_count > 1 {
            u64::from(renderer.rendering_parameters.path_sample_count)
        } else {
            0
        });
    assert!(
        allocated >= minimum,
        "measured{allocated}, expected at least{minimum}"
    );
    println!(
        "BLADE_LAZY_SCRATCH_PROBE: device={} dimensions=3840x2160 sample_count={} avoided_device_bytes={allocated}",
        renderer.gpu.device_information().device_name,
        renderer.rendering_parameters.path_sample_count
    );
    renderer.destroy();
    assert_eq!(renderer.scratch_texture_count(), 0);
}

#[cfg(feature = "custom-shaders")]
#[test]
fn blade_bounded_scene_fence_failures_retain_scratch_and_invalidate_owned_handles() {
    let program = crate::ShaderHandle::compile_fragment(crate::ShaderDescriptor::fragment(
        "recovery",
        "@fragment fn main()->@location(0) vec4<f32>{return vec4<f32>(0.0,1.0,0.0,1.0);}",
        "main",
    ))
    .unwrap();
    for failure in [
        Ok(false),
        Err(gpu::DeviceError::OutOfMemory),
        Err(gpu::DeviceError::DeviceLost),
    ] {
        let mut renderer = renderer(gpu::AlphaMode::PreMultiplied);
        let target = renderer
            .create_render_target(crate::RenderTargetDescriptor::rgba8(2, 2))
            .unwrap();
        renderer
            .render_shader(&target, &program, &crate::ShaderBindings::new())
            .unwrap();
        let buffer = renderer
            .create_gpu_buffer(crate::GpuBufferDescriptor { byte_len: 16 })
            .unwrap();
        let mut scene = Scene::default();
        scene.insert_primitive(path(16.0, hsla(0.0, 1.0, 0.5, 1.0)));
        scene.finish();
        renderer.render_scene_to_bgra(&scene).unwrap();
        let count = renderer.scratch_texture_count();
        assert!(count >= 1 && renderer.last_sync_point.is_some());
        let before = renderer.wait_calls;
        renderer.wait_override = Some(failure);
        renderer.update_drawable_size(size(DevicePixels(32), DevicePixels(32)));
        assert_eq!(
            renderer.wait_calls,
            before + 1,
            "one bounded attempt, never retry indefinitely"
        );
        assert_eq!(renderer.surface_config.size.width, 16);
        assert_eq!(renderer.scratch_texture_count(), count);
        assert!(renderer.last_sync_point.is_some());
        assert!(renderer.device_failed && !target.is_valid() && !buffer.is_valid());
        renderer.shed_memory(crate::MemoryPressureLevel::Critical);
        assert_eq!(
            renderer.scratch_texture_count(),
            count,
            "pressure cannot destroy in-flight scratch"
        );
        assert!(renderer.render_scene_to_bgra(&scene).is_err());
        assert!(matches!(
            renderer.create_gpu_buffer(crate::GpuBufferDescriptor { byte_len: 16 }),
            Err(crate::RenderTargetError::WrongDevice)
        ));
        renderer.wait_override = None;
        assert!(
            renderer.wait_for_gpu(),
            "retire the real completed submission"
        );
        renderer.shed_memory(crate::MemoryPressureLevel::Critical);
        assert_eq!(renderer.scratch_texture_count(), 0);
        renderer.destroy();
    }
    // CPU programs survive device generations and execute on recreated resources.
    let mut recreated = renderer(gpu::AlphaMode::PreMultiplied);
    let output = recreated
        .create_render_target(crate::RenderTargetDescriptor::rgba8(2, 2))
        .unwrap();
    recreated
        .render_shader(&output, &program, &crate::ShaderBindings::new())
        .unwrap();
    assert_eq!(
        recreated.read_render_target(&output).unwrap().pixels,
        [0, 255, 0, 255].repeat(4)
    );
    recreated.destroy();
}

#[test]
fn blade_readback_timeout_retains_staging_until_real_fence_completion() {
    let mut renderer = renderer(gpu::AlphaMode::PreMultiplied);
    renderer.wait_override = Some(Ok(false));
    let mut scene = Scene::default();
    scene.insert_primitive(quad(16.0, hsla(0.0, 1.0, 0.5, 1.0)));
    scene.finish();
    assert!(renderer.render_scene_to_bgra(&scene).is_err());
    assert_eq!(renderer.deferred_readbacks.len(), 1);
    assert!(renderer.last_sync_point.is_some() && renderer.device_failed);
    renderer.shed_memory(crate::MemoryPressureLevel::Critical);
    assert_eq!(renderer.deferred_readbacks.len(), 1);
    renderer.wait_override = None;
    assert!(renderer.wait_for_gpu());
    assert!(renderer.deferred_readbacks.is_empty());
    renderer.destroy();
}

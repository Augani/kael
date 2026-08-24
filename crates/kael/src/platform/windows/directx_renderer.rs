use std::{
    mem::ManuallyDrop,
    sync::{Arc, OnceLock},
};

use crate::util::ResultExt;
use anyhow::{Context, Result};
use windows::{
    Win32::{
        Foundation::HWND,
        Graphics::{
            Direct3D::*,
            Direct3D11::*,
            DirectComposition::*,
            DirectWrite::*,
            Dxgi::{Common::*, *},
        },
    },
    core::Interface,
};

use crate::{
    platform::windows::directx_renderer::shader_resources::{
        RawShaderBytes, ShaderModule, ShaderTarget,
    },
    *,
};

pub(crate) const DISABLE_DIRECT_COMPOSITION: &str = "GPUI_DISABLE_DIRECT_COMPOSITION";
const RENDER_TARGET_FORMAT: DXGI_FORMAT = DXGI_FORMAT_B8G8R8A8_UNORM;
// This configuration is used for MSAA rendering on paths only, and it's guaranteed to be supported by DirectX 11.
const PATH_MULTISAMPLE_COUNT: u32 = 4;
const MAX_SCENE_READBACK_BYTES: usize = 256 * 1024 * 1024;

pub(crate) struct DirectXSceneReadback {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) premultiplied_bgra: Vec<u8>,
}

fn require_com_output<T>(output: Option<T>, operation: &'static str) -> Result<T> {
    output.ok_or_else(|| anyhow::anyhow!("{operation} succeeded without returning an object"))
}

pub(crate) struct FontInfo {
    pub gamma_ratios: [f32; 4],
    pub grayscale_enhanced_contrast: f32,
}

pub(crate) struct DirectXRenderer {
    hwnd: HWND,
    atlas: Arc<DirectXAtlas>,
    devices: ManuallyDrop<DirectXRendererDevices>,
    resources: ManuallyDrop<DirectXResources>,
    globals: DirectXGlobalElements,
    pipelines: DirectXRenderPipelines,
    direct_composition: Option<DirectComposition>,
    font_info: &'static FontInfo,
    last_pipeline: Option<*const ()>,
    atlas_byte_budget: Option<u64>,
}

/// Direct3D objects
#[derive(Clone)]
pub(crate) struct DirectXRendererDevices {
    pub(crate) adapter: IDXGIAdapter1,
    pub(crate) dxgi_factory: IDXGIFactory6,
    pub(crate) device: ID3D11Device,
    pub(crate) device_context: ID3D11DeviceContext,
    dxgi_device: Option<IDXGIDevice>,
}

struct DirectXResources {
    // Direct3D rendering objects
    swap_chain: IDXGISwapChain1,
    render_target: Option<ID3D11Texture2D>,
    render_target_view: [Option<ID3D11RenderTargetView>; 1],

    // Path intermediate textures (with MSAA)
    path_intermediate_texture: ID3D11Texture2D,
    path_intermediate_srv: [Option<ID3D11ShaderResourceView>; 1],
    path_intermediate_msaa_texture: ID3D11Texture2D,
    path_intermediate_msaa_view: [Option<ID3D11RenderTargetView>; 1],
    cached_surface_texture: ID3D11Texture2D,
    cached_surface_view: [Option<ID3D11RenderTargetView>; 1],
    blur_source_texture: ID3D11Texture2D,
    blur_source_srv: [Option<ID3D11ShaderResourceView>; 1],
    blur_horizontal_texture: ID3D11Texture2D,
    blur_horizontal_srv: [Option<ID3D11ShaderResourceView>; 1],
    blur_horizontal_view: [Option<ID3D11RenderTargetView>; 1],

    // Cached window size and viewport
    width: u32,
    height: u32,
    viewport: [D3D11_VIEWPORT; 1],
}

struct DirectXRenderPipelines {
    blur_horizontal_pipeline: PipelineState<BlurPass>,
    blur_composite_pipeline: PipelineState<BlurPass>,
    shadow_pipeline: PipelineState<Shadow>,
    quad_pipeline: PipelineState<Quad>,
    path_rasterization_pipeline: PipelineState<PathRasterizationSprite>,
    path_sprite_pipeline: PipelineState<PathSprite>,
    underline_pipeline: PipelineState<Underline>,
    mono_sprites: PipelineState<MonochromeSprite>,
    poly_sprites: PipelineState<PolychromeSprite>,
}

struct DirectXGlobalElements {
    global_params_buffer: [Option<ID3D11Buffer>; 1],
    sampler: [Option<ID3D11SamplerState>; 1],
}

struct DirectComposition {
    comp_device: IDCompositionDevice,
    comp_target: IDCompositionTarget,
    comp_visual: IDCompositionVisual,
}

impl DirectXRendererDevices {
    pub(crate) fn new(
        directx_devices: &DirectXDevices,
        disable_direct_composition: bool,
    ) -> Result<ManuallyDrop<Self>> {
        let DirectXDevices {
            adapter,
            dxgi_factory,
            device,
            device_context,
        } = directx_devices;
        let dxgi_device = if disable_direct_composition {
            None
        } else {
            Some(device.cast().context("Creating DXGI device")?)
        };

        Ok(ManuallyDrop::new(Self {
            adapter: adapter.clone(),
            dxgi_factory: dxgi_factory.clone(),
            device: device.clone(),
            device_context: device_context.clone(),
            dxgi_device,
        }))
    }
}

impl DirectXRenderer {
    pub(crate) fn new(
        hwnd: HWND,
        directx_devices: &DirectXDevices,
        disable_direct_composition: bool,
    ) -> Result<Self> {
        if disable_direct_composition {
            log::info!("Direct Composition is disabled.");
        }

        let devices = DirectXRendererDevices::new(directx_devices, disable_direct_composition)
            .context("Creating DirectX devices")?;
        let atlas = Arc::new(DirectXAtlas::new(&devices.device, &devices.device_context));

        let resources = DirectXResources::new(&devices, 1, 1, hwnd, disable_direct_composition)
            .context("Creating DirectX resources")?;
        let globals = DirectXGlobalElements::new(&devices.device)
            .context("Creating DirectX global elements")?;
        let pipelines = DirectXRenderPipelines::new(&devices.device)
            .context("Creating DirectX render pipelines")?;

        let direct_composition = if disable_direct_composition {
            None
        } else {
            let dxgi_device = devices
                .dxgi_device
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("DirectComposition requires a DXGI device"))?;
            let composition =
                DirectComposition::new(dxgi_device, hwnd).context("Creating DirectComposition")?;
            composition
                .set_swap_chain(&resources.swap_chain)
                .context("Setting swap chain for DirectComposition")?;
            Some(composition)
        };

        Ok(DirectXRenderer {
            hwnd,
            atlas,
            devices,
            resources,
            globals,
            pipelines,
            direct_composition,
            font_info: Self::get_font_info(),
            last_pipeline: None,
            atlas_byte_budget: None,
        })
    }

    pub(crate) fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.atlas.clone()
    }

    fn pre_draw(&mut self) -> Result<()> {
        self.last_pipeline = None;
        let global_params_buffer = self.globals.global_params_buffer[0]
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("DirectX global parameter buffer is unavailable"))?;
        let render_target_view = self.resources.render_target_view[0]
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("DirectX render-target view is unavailable"))?;
        update_buffer(
            &self.devices.device_context,
            global_params_buffer,
            &[GlobalParams {
                gamma_ratios: self.font_info.gamma_ratios,
                viewport_size: [
                    self.resources.viewport[0].Width,
                    self.resources.viewport[0].Height,
                ],
                grayscale_enhanced_contrast: self.font_info.grayscale_enhanced_contrast,
                _pad: 0,
            }],
        )?;
        unsafe {
            self.devices
                .device_context
                .ClearRenderTargetView(render_target_view, &[0.0; 4]);
            self.devices
                .device_context
                .OMSetRenderTargets(Some(&self.resources.render_target_view), None);
            self.devices
                .device_context
                .RSSetViewports(Some(&self.resources.viewport));
        }
        Ok(())
    }

    #[inline]
    fn present(&mut self) -> Result<()> {
        let result = unsafe { self.resources.swap_chain.Present(0, DXGI_PRESENT(0)) };
        result.ok().context("Presenting swap chain failed")
    }

    pub(crate) fn handle_device_lost(&mut self, directx_devices: &DirectXDevices) {
        try_to_recover_from_device_lost(
            || {
                self.handle_device_lost_impl(directx_devices)
                    .context("DirectXRenderer handling device lost")
            },
            |_| {},
            || {
                log::error!(
                    "DirectXRenderer failed to recover from device lost after multiple attempts"
                );
                // Do something here?
                // At this point, the device loss is considered unrecoverable.
            },
        );
    }

    fn handle_device_lost_impl(&mut self, directx_devices: &DirectXDevices) -> Result<()> {
        let disable_direct_composition = self.direct_composition.is_none();
        let width = self.resources.width;
        let height = self.resources.height;

        let devices = DirectXRendererDevices::new(directx_devices, disable_direct_composition)
            .context("Recreating DirectX devices")?;
        let resources = DirectXResources::new(
            &devices,
            width,
            height,
            self.hwnd,
            disable_direct_composition,
        )?;
        let globals = DirectXGlobalElements::new(&devices.device)?;
        let pipelines = DirectXRenderPipelines::new(&devices.device)?;

        let direct_composition = if disable_direct_composition {
            None
        } else {
            let dxgi_device = devices.dxgi_device.as_ref().ok_or_else(|| {
                anyhow::anyhow!("DirectComposition recovery requires a DXGI device")
            })?;
            let composition = DirectComposition::new(dxgi_device, self.hwnd)?;
            composition.set_swap_chain(&resources.swap_chain)?;
            Some(composition)
        };

        // Build the replacement state first. If any step above fails, the current
        // renderer remains structurally valid for the recovery loop to retry.
        unsafe {
            #[cfg(debug_assertions)]
            report_live_objects(&self.devices.device)
                .context("Failed to report live objects after device lost")
                .log_err();

            self.devices.device_context.OMSetRenderTargets(None, None);
            self.devices.device_context.ClearState();
            self.devices.device_context.Flush();
            drop(self.direct_composition.take());
            ManuallyDrop::drop(&mut self.resources);

            #[cfg(debug_assertions)]
            report_live_objects(&self.devices.device)
                .context("Failed to report live objects after device lost")
                .log_err();

            ManuallyDrop::drop(&mut self.devices);
        }

        self.atlas
            .handle_device_lost(&devices.device, &devices.device_context);
        self.devices = devices;
        self.resources = resources;
        self.globals = globals;
        self.pipelines = pipelines;
        self.direct_composition = direct_composition;

        unsafe {
            self.devices
                .device_context
                .OMSetRenderTargets(Some(&self.resources.render_target_view), None);
        }
        Ok(())
    }

    fn render_scene(&mut self, scene: &Scene) -> Result<()> {
        self.pre_draw()?;
        for batch in scene.batches() {
            match batch {
                PrimitiveBatch::Shadows(shadows) => self.draw_shadows(shadows),
                PrimitiveBatch::BlurRects(blur_rects) => self.draw_blur_rects(blur_rects),
                PrimitiveBatch::Quads(quads) => self.draw_quads(quads),
                PrimitiveBatch::Paths(paths) => {
                    let target_view = self.resources.render_target_view.clone();
                    self.draw_paths_to_intermediate(paths, &target_view)?;
                    self.draw_paths_from_intermediate(paths)
                }
                PrimitiveBatch::Underlines(underlines) => self.draw_underlines(underlines),
                PrimitiveBatch::MonochromeSprites {
                    texture_id,
                    sprites,
                } => self.draw_monochrome_sprites(texture_id, sprites),
                PrimitiveBatch::PolychromeSprites {
                    texture_id,
                    sprites,
                } => self.draw_polychrome_sprites(texture_id, sprites),
                PrimitiveBatch::Surfaces(surfaces) => self.draw_surfaces(surfaces),
            }.context(format!("scene too large: {} paths, {} shadows, {} quads, {} underlines, {} mono, {} poly, {} surfaces",
                    scene.paths.len(),
                    scene.shadows.len(),
                    scene.quads.len(),
                    scene.underlines.len(),
                    scene.monochrome_sprites.len(),
                    scene.polychrome_sprites.len(),
                    scene.surfaces.len(),))?;
        }

        self.draw_cached_surface_snapshots(scene)?;
        Ok(())
    }

    pub(crate) fn draw(&mut self, scene: &Scene) -> Result<()> {
        self.render_scene(scene)?;
        self.present()?;
        if let Some(budget) = self.atlas_byte_budget {
            const IN_FLIGHT_FRAMES: u64 = 3;
            self.atlas.evict_to_budget_keeping(budget, IN_FLIGHT_FRAMES);
        }
        self.atlas.advance_frame();
        Ok(())
    }

    /// Render a retained Kael scene and copy the exact device-pixel target into
    /// bounded CPU memory. This intentionally reads the renderer target rather
    /// than the HWND so OS chrome, the cursor, and hosted child windows are not
    /// silently included.
    pub(crate) fn render_scene_to_bgra(&mut self, scene: &Scene) -> Result<DirectXSceneReadback> {
        self.render_scene(scene)?;

        let width = self.resources.width;
        let height = self.resources.height;
        anyhow::ensure!(width > 0 && height > 0, "scene readback target is empty");
        let row_bytes = usize::try_from(width)
            .ok()
            .and_then(|width| width.checked_mul(4))
            .context("scene readback row byte count overflowed")?;
        let byte_len = usize::try_from(height)
            .ok()
            .and_then(|height| height.checked_mul(row_bytes))
            .context("scene readback byte count overflowed")?;
        anyhow::ensure!(
            byte_len <= MAX_SCENE_READBACK_BYTES,
            "scene readback exceeds the {MAX_SCENE_READBACK_BYTES}-byte safety limit"
        );

        let staging = {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
                MipLevels: 1,
                ArraySize: 1,
                Format: RENDER_TARGET_FORMAT,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
            };
            let mut staging = None;
            unsafe {
                self.devices
                    .device
                    .CreateTexture2D(&desc, None, Some(&mut staging))
            }
            .context("creating Direct3D scene-readback staging texture")?;
            require_com_output(staging, "CreateTexture2D for scene readback")?
        };

        let render_target = self
            .resources
            .render_target
            .as_ref()
            .context("Direct3D scene readback target is unavailable")?;
        unsafe {
            // Detach the target before copying so the runtime never observes it
            // simultaneously bound for output and used as a copy source.
            self.devices.device_context.OMSetRenderTargets(None, None);
            self.devices
                .device_context
                .CopyResource(&staging, render_target);
        }

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        let map_result = unsafe {
            self.devices
                .device_context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
        };
        if let Err(error) = map_result {
            // Copy setup temporarily detached the target. Restore it even when
            // the staging texture cannot be mapped so the live window remains
            // renderable after a failed export.
            unsafe {
                self.devices
                    .device_context
                    .OMSetRenderTargets(Some(&self.resources.render_target_view), None);
            }
            return Err(error).context("mapping Direct3D scene-readback staging texture");
        }

        let copy_result = (|| -> Result<Vec<u8>> {
            let row_pitch = usize::try_from(mapped.RowPitch)
                .context("Direct3D scene-readback row pitch does not fit usize")?;
            anyhow::ensure!(
                row_pitch >= row_bytes,
                "Direct3D scene-readback row pitch is smaller than a pixel row"
            );
            anyhow::ensure!(
                !mapped.pData.is_null(),
                "Direct3D mapped scene-readback pointer is null"
            );
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(byte_len)
                .context("allocating Direct3D scene-readback buffer")?;
            bytes.resize(byte_len, 0);
            for row in 0..usize::try_from(height).unwrap_or(0) {
                let source_offset = row
                    .checked_mul(row_pitch)
                    .context("Direct3D source row offset overflowed")?;
                let destination_offset = row
                    .checked_mul(row_bytes)
                    .context("Direct3D destination row offset overflowed")?;
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        (mapped.pData as *const u8).add(source_offset),
                        bytes.as_mut_ptr().add(destination_offset),
                        row_bytes,
                    );
                }
            }
            Ok(bytes)
        })();
        unsafe {
            self.devices.device_context.Unmap(&staging, 0);
            self.devices
                .device_context
                .OMSetRenderTargets(Some(&self.resources.render_target_view), None);
        }
        let premultiplied_bgra = copy_result?;

        Ok(DirectXSceneReadback {
            width,
            height,
            premultiplied_bgra,
        })
    }

    pub(crate) fn set_atlas_byte_budget(&mut self, budget: Option<u64>) {
        self.atlas_byte_budget = budget;
    }

    fn draw_cached_surface_snapshots(&mut self, scene: &Scene) -> Result<()> {
        for snapshot in &scene.cached_surface_snapshots {
            let cached_view = self.resources.cached_surface_view.clone();
            self.bind_render_target(&cached_view, true)?;

            let snapshot_scene = scene.snapshot_subscene(snapshot.paint_operations.clone());
            for batch in snapshot_scene.batches() {
                match batch {
                    PrimitiveBatch::Shadows(shadows) => self.draw_shadows(shadows),
                    PrimitiveBatch::BlurRects(blur_rects) => {
                        self.draw_blur_rects_to_cached_surface(blur_rects)
                    }
                    PrimitiveBatch::Quads(quads) => self.draw_quads(quads),
                    PrimitiveBatch::Paths(paths) => {
                        self.draw_paths_to_intermediate(paths, &cached_view)?;
                        self.draw_paths_from_intermediate(paths)
                    }
                    PrimitiveBatch::Underlines(underlines) => self.draw_underlines(underlines),
                    PrimitiveBatch::MonochromeSprites {
                        texture_id,
                        sprites,
                    } => self.draw_monochrome_sprites(texture_id, sprites),
                    PrimitiveBatch::PolychromeSprites {
                        texture_id,
                        sprites,
                    } => self.draw_polychrome_sprites(texture_id, sprites),
                    PrimitiveBatch::Surfaces(surfaces) => self.draw_surfaces(surfaces),
                }
                .context("cached surface snapshot scene too large")?;
            }

            let destination_texture = self.atlas.get_texture(snapshot.target.texture_id)?;
            let source_box = D3D11_BOX {
                left: snapshot.source_bounds.origin.x.0 as u32,
                top: snapshot.source_bounds.origin.y.0 as u32,
                front: 0,
                right: (snapshot.source_bounds.origin.x.0 + snapshot.source_bounds.size.width.0)
                    as u32,
                bottom: (snapshot.source_bounds.origin.y.0 + snapshot.source_bounds.size.height.0)
                    as u32,
                back: 1,
            };

            unsafe {
                self.devices.device_context.CopySubresourceRegion(
                    &destination_texture,
                    0,
                    snapshot.target.bounds.origin.x.0 as u32,
                    snapshot.target.bounds.origin.y.0 as u32,
                    0,
                    &self.resources.cached_surface_texture,
                    0,
                    Some(&source_box),
                );
            }
        }

        unsafe {
            self.devices
                .device_context
                .OMSetRenderTargets(Some(&self.resources.render_target_view), None);
        }

        Ok(())
    }

    fn bind_render_target(
        &mut self,
        render_target_view: &[Option<ID3D11RenderTargetView>; 1],
        clear: bool,
    ) -> Result<()> {
        let target_view = render_target_view[0]
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("DirectX render-target view is unavailable"))?;
        unsafe {
            self.devices
                .device_context
                .OMSetRenderTargets(Some(render_target_view), None);
            self.devices
                .device_context
                .RSSetViewports(Some(&self.resources.viewport));
            if clear {
                self.devices
                    .device_context
                    .ClearRenderTargetView(target_view, &[0.0; 4]);
            }
        }

        self.last_pipeline = None;
        Ok(())
    }

    pub(crate) fn resize(&mut self, new_size: Size<DevicePixels>) -> Result<()> {
        let width = new_size
            .width
            .0
            .clamp(1, crate::MAX_ATLAS_TEXTURE_DIMENSION) as u32;
        let height = new_size
            .height
            .0
            .clamp(1, crate::MAX_ATLAS_TEXTURE_DIMENSION) as u32;
        if width != new_size.width.0.max(0) as u32 || height != new_size.height.0.max(0) as u32 {
            log::warn!(
                "clamping unsafe DirectX drawable size {:?} to {width}x{height}",
                new_size
            );
        }
        if self.resources.width == width && self.resources.height == height {
            return Ok(());
        }
        // Clear the render target before resizing
        unsafe { self.devices.device_context.OMSetRenderTargets(None, None) };
        self.resources.render_target.take();
        self.resources.render_target_view[0].take();

        // Resizing the swap chain requires a call to the underlying DXGI adapter, which can return the device removed error.
        // The app might have moved to a monitor that's attached to a different graphics device.
        // When a graphics device is removed or reset, the desktop resolution often changes, resulting in a window size change.
        // But here we just return the error, because we are handling device lost scenarios elsewhere.
        let resize_result = unsafe {
            self.resources.swap_chain.ResizeBuffers(
                BUFFER_COUNT as u32,
                width,
                height,
                RENDER_TARGET_FORMAT,
                DXGI_SWAP_CHAIN_FLAG(0),
            )
        };
        if let Err(error) = resize_result {
            if let Ok((render_target, render_target_view)) =
                create_render_target_and_its_view(&self.resources.swap_chain, &self.devices.device)
            {
                self.resources.render_target = Some(render_target);
                self.resources.render_target_view = render_target_view;
            }
            return Err(error).context("Failed to resize swap chain");
        }

        self.resources
            .recreate_resources(&self.devices, width, height)?;
        self.resources.width = width;
        self.resources.height = height;
        unsafe {
            self.devices
                .device_context
                .OMSetRenderTargets(Some(&self.resources.render_target_view), None);
        }

        Ok(())
    }

    fn draw_shadows(&mut self, shadows: &[Shadow]) -> Result<()> {
        if shadows.is_empty() {
            return Ok(());
        }
        self.pipelines.shadow_pipeline.update_buffer(
            &self.devices.device,
            &self.devices.device_context,
            shadows,
            &mut self.last_pipeline,
        )?;
        self.pipelines.shadow_pipeline.draw(
            &self.devices.device_context,
            &self.resources.viewport,
            &self.globals.global_params_buffer,
            D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
            4,
            shadows.len() as u32,
            &mut self.last_pipeline,
        )
    }

    fn draw_quads(&mut self, quads: &[Quad]) -> Result<()> {
        if quads.is_empty() {
            return Ok(());
        }
        self.pipelines.quad_pipeline.update_buffer(
            &self.devices.device,
            &self.devices.device_context,
            quads,
            &mut self.last_pipeline,
        )?;
        self.pipelines.quad_pipeline.draw(
            &self.devices.device_context,
            &self.resources.viewport,
            &self.globals.global_params_buffer,
            D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
            4,
            quads.len() as u32,
            &mut self.last_pipeline,
        )
    }

    fn draw_paths_to_intermediate(
        &mut self,
        paths: &[Path<ScaledPixels>],
        restore_target: &[Option<ID3D11RenderTargetView>; 1],
    ) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }

        // Render target change invalidates pipeline state cache
        self.last_pipeline = None;

        // Clear intermediate MSAA texture
        let intermediate_view = self.resources.path_intermediate_msaa_view[0]
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("DirectX path render-target view is unavailable"))?;
        unsafe {
            self.devices
                .device_context
                .ClearRenderTargetView(intermediate_view, &[0.0; 4]);
            // Set intermediate MSAA texture as render target
            self.devices
                .device_context
                .OMSetRenderTargets(Some(&self.resources.path_intermediate_msaa_view), None);
        }

        // Collect all vertices and sprites for a single draw call
        let mut vertices = Vec::new();

        for path in paths {
            vertices.extend(path.vertices.iter().map(|v| PathRasterizationSprite {
                xy_position: v.xy_position,
                st_position: v.st_position,
                color: path.color,
                bounds: path.clipped_bounds(),
            }));
        }

        self.pipelines.path_rasterization_pipeline.update_buffer(
            &self.devices.device,
            &self.devices.device_context,
            &vertices,
            &mut self.last_pipeline,
        )?;
        self.pipelines.path_rasterization_pipeline.draw(
            &self.devices.device_context,
            &self.resources.viewport,
            &self.globals.global_params_buffer,
            D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
            vertices.len() as u32,
            1,
            &mut self.last_pipeline,
        )?;

        // Resolve MSAA to non-MSAA intermediate texture
        unsafe {
            self.devices.device_context.ResolveSubresource(
                &self.resources.path_intermediate_texture,
                0,
                &self.resources.path_intermediate_msaa_texture,
                0,
                RENDER_TARGET_FORMAT,
            );
            // Restore main render target — invalidates pipeline state cache
            self.last_pipeline = None;
            self.devices
                .device_context
                .OMSetRenderTargets(Some(restore_target), None);
        }

        Ok(())
    }

    fn draw_paths_from_intermediate(&mut self, paths: &[Path<ScaledPixels>]) -> Result<()> {
        let Some(first_path) = paths.first() else {
            return Ok(());
        };

        // When copying paths from the intermediate texture to the drawable,
        // each pixel must only be copied once, in case of transparent paths.
        //
        // If all paths have the same draw order, then their bounds are all
        // disjoint, so we can copy each path's bounds individually. If this
        // batch combines different draw orders, we perform a single copy
        // for a minimal spanning rect.
        let sprites = if paths
            .last()
            .is_some_and(|path| path.order == first_path.order)
        {
            paths
                .iter()
                .map(|path| PathSprite {
                    bounds: path.clipped_bounds(),
                })
                .collect::<Vec<_>>()
        } else {
            let mut bounds = first_path.clipped_bounds();
            for path in paths.iter().skip(1) {
                bounds = bounds.union(&path.clipped_bounds());
            }
            vec![PathSprite { bounds }]
        };

        self.pipelines.path_sprite_pipeline.update_buffer(
            &self.devices.device,
            &self.devices.device_context,
            &sprites,
            &mut self.last_pipeline,
        )?;

        // Draw the sprites with the path texture
        self.pipelines.path_sprite_pipeline.draw_with_texture(
            &self.devices.device_context,
            &self.resources.path_intermediate_srv,
            &self.resources.viewport,
            &self.globals.global_params_buffer,
            &self.globals.sampler,
            sprites.len() as u32,
            &mut self.last_pipeline,
        )
    }

    fn draw_underlines(&mut self, underlines: &[Underline]) -> Result<()> {
        if underlines.is_empty() {
            return Ok(());
        }
        self.pipelines.underline_pipeline.update_buffer(
            &self.devices.device,
            &self.devices.device_context,
            underlines,
            &mut self.last_pipeline,
        )?;
        self.pipelines.underline_pipeline.draw(
            &self.devices.device_context,
            &self.resources.viewport,
            &self.globals.global_params_buffer,
            D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
            4,
            underlines.len() as u32,
            &mut self.last_pipeline,
        )
    }

    fn draw_monochrome_sprites(
        &mut self,
        texture_id: AtlasTextureId,
        sprites: &[MonochromeSprite],
    ) -> Result<()> {
        if sprites.is_empty() {
            return Ok(());
        }
        self.pipelines.mono_sprites.update_buffer(
            &self.devices.device,
            &self.devices.device_context,
            sprites,
            &mut self.last_pipeline,
        )?;
        let texture_view = self.atlas.get_texture_view(texture_id)?;
        self.pipelines.mono_sprites.draw_with_texture(
            &self.devices.device_context,
            &texture_view,
            &self.resources.viewport,
            &self.globals.global_params_buffer,
            &self.globals.sampler,
            sprites.len() as u32,
            &mut self.last_pipeline,
        )
    }

    fn draw_polychrome_sprites(
        &mut self,
        texture_id: AtlasTextureId,
        sprites: &[PolychromeSprite],
    ) -> Result<()> {
        if sprites.is_empty() {
            return Ok(());
        }
        self.pipelines.poly_sprites.update_buffer(
            &self.devices.device,
            &self.devices.device_context,
            sprites,
            &mut self.last_pipeline,
        )?;
        let texture_view = self.atlas.get_texture_view(texture_id)?;
        self.pipelines.poly_sprites.draw_with_texture(
            &self.devices.device_context,
            &texture_view,
            &self.resources.viewport,
            &self.globals.global_params_buffer,
            &self.globals.sampler,
            sprites.len() as u32,
            &mut self.last_pipeline,
        )
    }

    fn draw_blur_rects(&mut self, blur_rects: &[BlurRect]) -> Result<()> {
        let target_texture = self
            .resources
            .render_target
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("swap-chain render target is unavailable"))?
            .clone();
        let target_view = [Some(
            self.resources.render_target_view[0]
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("DirectX render-target view is unavailable"))?
                .clone(),
        )];
        self.draw_blur_rects_with_target(blur_rects, &target_texture, &target_view)
    }

    fn draw_blur_rects_to_cached_surface(&mut self, blur_rects: &[BlurRect]) -> Result<()> {
        let target_texture = self.resources.cached_surface_texture.clone();
        let target_view = [Some(
            self.resources.cached_surface_view[0]
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("DirectX cached-surface view is unavailable"))?
                .clone(),
        )];
        self.draw_blur_rects_with_target(blur_rects, &target_texture, &target_view)
    }

    fn draw_blur_rects_with_target(
        &mut self,
        blur_rects: &[BlurRect],
        target_texture: &ID3D11Texture2D,
        target_view: &[Option<ID3D11RenderTargetView>; 1],
    ) -> Result<()> {
        if blur_rects.is_empty() {
            return Ok(());
        }

        let viewport_size = Size {
            width: DevicePixels(self.resources.width as i32),
            height: DevicePixels(self.resources.height as i32),
        };

        for blur_rect in blur_rects {
            let capture_bounds = blur_rect.capture_bounds(viewport_size);
            if capture_bounds.is_empty() {
                continue;
            }

            let source_box = D3D11_BOX {
                left: capture_bounds.origin.x.0 as u32,
                top: capture_bounds.origin.y.0 as u32,
                front: 0,
                right: (capture_bounds.origin.x.0 + capture_bounds.size.width.0) as u32,
                bottom: (capture_bounds.origin.y.0 + capture_bounds.size.height.0) as u32,
                back: 1,
            };

            self.unbind_shader_resources();
            unsafe {
                self.devices.device_context.CopySubresourceRegion(
                    &self.resources.blur_source_texture,
                    0,
                    capture_bounds.origin.x.0 as u32,
                    capture_bounds.origin.y.0 as u32,
                    0,
                    target_texture,
                    0,
                    Some(&source_box),
                );
            }

            let blur_h_view = self.resources.blur_horizontal_view.clone();
            self.bind_render_target(&blur_h_view, true)?;
            self.pipelines.blur_horizontal_pipeline.update_buffer(
                &self.devices.device,
                &self.devices.device_context,
                &[BlurPass::horizontal(blur_rect, capture_bounds)],
                &mut self.last_pipeline,
            )?;
            self.pipelines.blur_horizontal_pipeline.draw_with_texture(
                &self.devices.device_context,
                &self.resources.blur_source_srv,
                &self.resources.viewport,
                &self.globals.global_params_buffer,
                &self.globals.sampler,
                1,
                &mut self.last_pipeline,
            )?;

            self.bind_render_target(target_view, false)?;
            self.pipelines.blur_composite_pipeline.update_buffer(
                &self.devices.device,
                &self.devices.device_context,
                &[BlurPass::composite(blur_rect, capture_bounds)],
                &mut self.last_pipeline,
            )?;
            self.pipelines.blur_composite_pipeline.draw_with_texture(
                &self.devices.device_context,
                &self.resources.blur_horizontal_srv,
                &self.resources.viewport,
                &self.globals.global_params_buffer,
                &self.globals.sampler,
                1,
                &mut self.last_pipeline,
            )?;
        }

        Ok(())
    }

    fn draw_surfaces(&mut self, surfaces: &[PaintSurface]) -> Result<()> {
        if surfaces.is_empty() {
            return Ok(());
        }
        Ok(())
    }

    pub(crate) fn gpu_specs(&self) -> Result<GpuSpecs> {
        let desc = unsafe { self.devices.adapter.GetDesc1() }?;
        let is_software_emulated = (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0;
        let device_name = String::from_utf16_lossy(&desc.Description)
            .trim_matches(char::from(0))
            .to_string();
        let driver_name = match desc.VendorId {
            0x10DE => "NVIDIA Corporation".to_string(),
            0x1002 => "AMD Corporation".to_string(),
            0x8086 => "Intel Corporation".to_string(),
            id => format!("Unknown Vendor (ID: {:#X})", id),
        };
        let driver_version = match desc.VendorId {
            0x10DE => nvidia::get_driver_version(),
            0x1002 => amd::get_driver_version(),
            // For Intel and other vendors, we use the DXGI API to get the driver version.
            _ => dxgi::get_driver_version(&self.devices.adapter),
        }
        .context("Failed to get gpu driver info")
        .log_err()
        .unwrap_or("Unknown Driver".to_string());
        Ok(GpuSpecs {
            is_software_emulated,
            device_name,
            driver_name,
            driver_info: driver_version,
        })
    }

    pub(crate) fn get_font_info() -> &'static FontInfo {
        static CACHED_FONT_INFO: OnceLock<FontInfo> = OnceLock::new();
        CACHED_FONT_INFO.get_or_init(|| {
            let detected = (|| -> Result<FontInfo> {
                let factory: IDWriteFactory5 =
                    unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }
                        .context("creating DirectWrite factory for font rendering")?;
                let render_params: IDWriteRenderingParams1 = unsafe {
                    factory
                        .CreateRenderingParams()
                        .context("reading DirectWrite rendering parameters")?
                        .cast()
                        .context("querying DirectWrite rendering parameters v1")?
                };
                Ok(FontInfo {
                    gamma_ratios: Self::get_gamma_ratios(unsafe { render_params.GetGamma() }),
                    grayscale_enhanced_contrast: unsafe {
                        render_params.GetGrayscaleEnhancedContrast()
                    },
                })
            })();
            detected.unwrap_or_else(|error| {
                log::warn!("using conservative font-rendering defaults: {error:#}");
                FontInfo {
                    gamma_ratios: Self::get_gamma_ratios(2.2),
                    grayscale_enhanced_contrast: 1.0,
                }
            })
        })
    }

    // Gamma ratios for brightening/darkening edges for better contrast
    // https://github.com/microsoft/terminal/blob/1283c0f5b99a2961673249fa77c6b986efb5086c/src/renderer/atlas/dwrite.cpp#L50
    fn get_gamma_ratios(gamma: f32) -> [f32; 4] {
        const GAMMA_INCORRECT_TARGET_RATIOS: [[f32; 4]; 13] = [
            [0.0000 / 4.0, 0.0000 / 4.0, 0.0000 / 4.0, 0.0000 / 4.0], // gamma = 1.0
            [0.0166 / 4.0, -0.0807 / 4.0, 0.2227 / 4.0, -0.0751 / 4.0], // gamma = 1.1
            [0.0350 / 4.0, -0.1760 / 4.0, 0.4325 / 4.0, -0.1370 / 4.0], // gamma = 1.2
            [0.0543 / 4.0, -0.2821 / 4.0, 0.6302 / 4.0, -0.1876 / 4.0], // gamma = 1.3
            [0.0739 / 4.0, -0.3963 / 4.0, 0.8167 / 4.0, -0.2287 / 4.0], // gamma = 1.4
            [0.0933 / 4.0, -0.5161 / 4.0, 0.9926 / 4.0, -0.2616 / 4.0], // gamma = 1.5
            [0.1121 / 4.0, -0.6395 / 4.0, 1.1588 / 4.0, -0.2877 / 4.0], // gamma = 1.6
            [0.1300 / 4.0, -0.7649 / 4.0, 1.3159 / 4.0, -0.3080 / 4.0], // gamma = 1.7
            [0.1469 / 4.0, -0.8911 / 4.0, 1.4644 / 4.0, -0.3234 / 4.0], // gamma = 1.8
            [0.1627 / 4.0, -1.0170 / 4.0, 1.6051 / 4.0, -0.3347 / 4.0], // gamma = 1.9
            [0.1773 / 4.0, -1.1420 / 4.0, 1.7385 / 4.0, -0.3426 / 4.0], // gamma = 2.0
            [0.1908 / 4.0, -1.2652 / 4.0, 1.8650 / 4.0, -0.3476 / 4.0], // gamma = 2.1
            [0.2031 / 4.0, -1.3864 / 4.0, 1.9851 / 4.0, -0.3501 / 4.0], // gamma = 2.2
        ];

        const NORM13: f32 = ((0x10000 as f64) / (255.0 * 255.0) * 4.0) as f32;
        const NORM24: f32 = ((0x100 as f64) / (255.0) * 4.0) as f32;

        let index = ((gamma * 10.0).round() as usize).clamp(10, 22) - 10;
        let ratios = GAMMA_INCORRECT_TARGET_RATIOS[index];

        [
            ratios[0] * NORM13,
            ratios[1] * NORM24,
            ratios[2] * NORM13,
            ratios[3] * NORM24,
        ]
    }

    fn unbind_shader_resources(&self) {
        let null_srv = [None];
        unsafe {
            self.devices
                .device_context
                .VSSetShaderResources(0, Some(&null_srv));
            self.devices
                .device_context
                .PSSetShaderResources(0, Some(&null_srv));
        }
    }
}

impl DirectXResources {
    pub fn new(
        devices: &DirectXRendererDevices,
        width: u32,
        height: u32,
        hwnd: HWND,
        disable_direct_composition: bool,
    ) -> Result<ManuallyDrop<Self>> {
        let swap_chain = if disable_direct_composition {
            create_swap_chain(&devices.dxgi_factory, &devices.device, hwnd, width, height)?
        } else {
            create_swap_chain_for_composition(
                &devices.dxgi_factory,
                &devices.device,
                width,
                height,
            )?
        };

        let (
            render_target,
            render_target_view,
            path_intermediate_texture,
            path_intermediate_srv,
            path_intermediate_msaa_texture,
            path_intermediate_msaa_view,
            cached_surface_texture,
            cached_surface_view,
            blur_source_texture,
            blur_source_srv,
            blur_horizontal_texture,
            blur_horizontal_srv,
            blur_horizontal_view,
            viewport,
        ) = create_resources(devices, &swap_chain, width, height)?;
        set_rasterizer_state(&devices.device, &devices.device_context)?;

        Ok(ManuallyDrop::new(Self {
            swap_chain,
            render_target: Some(render_target),
            render_target_view,
            path_intermediate_texture,
            path_intermediate_msaa_texture,
            path_intermediate_msaa_view,
            path_intermediate_srv,
            cached_surface_texture,
            cached_surface_view,
            blur_source_texture,
            blur_source_srv,
            blur_horizontal_texture,
            blur_horizontal_srv,
            blur_horizontal_view,
            viewport,
            width,
            height,
        }))
    }

    #[inline]
    fn recreate_resources(
        &mut self,
        devices: &DirectXRendererDevices,
        width: u32,
        height: u32,
    ) -> Result<()> {
        let (
            render_target,
            render_target_view,
            path_intermediate_texture,
            path_intermediate_srv,
            path_intermediate_msaa_texture,
            path_intermediate_msaa_view,
            cached_surface_texture,
            cached_surface_view,
            blur_source_texture,
            blur_source_srv,
            blur_horizontal_texture,
            blur_horizontal_srv,
            blur_horizontal_view,
            viewport,
        ) = create_resources(devices, &self.swap_chain, width, height)?;
        self.render_target = Some(render_target);
        self.render_target_view = render_target_view;
        self.path_intermediate_texture = path_intermediate_texture;
        self.path_intermediate_msaa_texture = path_intermediate_msaa_texture;
        self.path_intermediate_msaa_view = path_intermediate_msaa_view;
        self.path_intermediate_srv = path_intermediate_srv;
        self.cached_surface_texture = cached_surface_texture;
        self.cached_surface_view = cached_surface_view;
        self.blur_source_texture = blur_source_texture;
        self.blur_source_srv = blur_source_srv;
        self.blur_horizontal_texture = blur_horizontal_texture;
        self.blur_horizontal_srv = blur_horizontal_srv;
        self.blur_horizontal_view = blur_horizontal_view;
        self.viewport = viewport;
        Ok(())
    }
}

impl DirectXRenderPipelines {
    pub fn new(device: &ID3D11Device) -> Result<Self> {
        let blur_horizontal_pipeline = PipelineState::new(
            device,
            "blur_horizontal_pipeline",
            ShaderModule::BlurHorizontal,
            1,
            create_blend_state(device)?,
        )?;
        let blur_composite_pipeline = PipelineState::new(
            device,
            "blur_composite_pipeline",
            ShaderModule::BlurComposite,
            1,
            create_blend_state(device)?,
        )?;
        let shadow_pipeline = PipelineState::new(
            device,
            "shadow_pipeline",
            ShaderModule::Shadow,
            4,
            create_blend_state(device)?,
        )?;
        let quad_pipeline = PipelineState::new(
            device,
            "quad_pipeline",
            ShaderModule::Quad,
            64,
            create_blend_state(device)?,
        )?;
        let path_rasterization_pipeline = PipelineState::new(
            device,
            "path_rasterization_pipeline",
            ShaderModule::PathRasterization,
            32,
            create_blend_state_for_path_rasterization(device)?,
        )?;
        let path_sprite_pipeline = PipelineState::new(
            device,
            "path_sprite_pipeline",
            ShaderModule::PathSprite,
            4,
            create_blend_state_for_path_sprite(device)?,
        )?;
        let underline_pipeline = PipelineState::new(
            device,
            "underline_pipeline",
            ShaderModule::Underline,
            4,
            create_blend_state(device)?,
        )?;
        let mono_sprites = PipelineState::new(
            device,
            "monochrome_sprite_pipeline",
            ShaderModule::MonochromeSprite,
            512,
            create_blend_state(device)?,
        )?;
        let poly_sprites = PipelineState::new(
            device,
            "polychrome_sprite_pipeline",
            ShaderModule::PolychromeSprite,
            16,
            create_blend_state(device)?,
        )?;

        Ok(Self {
            blur_horizontal_pipeline,
            blur_composite_pipeline,
            shadow_pipeline,
            quad_pipeline,
            path_rasterization_pipeline,
            path_sprite_pipeline,
            underline_pipeline,
            mono_sprites,
            poly_sprites,
        })
    }
}

impl DirectComposition {
    pub fn new(dxgi_device: &IDXGIDevice, hwnd: HWND) -> Result<Self> {
        let comp_device = get_comp_device(dxgi_device)?;
        let comp_target = unsafe { comp_device.CreateTargetForHwnd(hwnd, true) }?;
        let comp_visual = unsafe { comp_device.CreateVisual() }?;

        Ok(Self {
            comp_device,
            comp_target,
            comp_visual,
        })
    }

    pub fn set_swap_chain(&self, swap_chain: &IDXGISwapChain1) -> Result<()> {
        unsafe {
            self.comp_visual.SetContent(swap_chain)?;
            self.comp_target.SetRoot(&self.comp_visual)?;
            self.comp_device.Commit()?;
        }
        Ok(())
    }
}

impl DirectXGlobalElements {
    pub fn new(device: &ID3D11Device) -> Result<Self> {
        let global_params_buffer = unsafe {
            let desc = D3D11_BUFFER_DESC {
                ByteWidth: std::mem::size_of::<GlobalParams>() as u32,
                Usage: D3D11_USAGE_DYNAMIC,
                BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                ..Default::default()
            };
            let mut buffer = None;
            device.CreateBuffer(&desc, None, Some(&mut buffer))?;
            [buffer]
        };

        let sampler = unsafe {
            let desc = D3D11_SAMPLER_DESC {
                Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                AddressU: D3D11_TEXTURE_ADDRESS_WRAP,
                AddressV: D3D11_TEXTURE_ADDRESS_WRAP,
                AddressW: D3D11_TEXTURE_ADDRESS_WRAP,
                MipLODBias: 0.0,
                MaxAnisotropy: 1,
                ComparisonFunc: D3D11_COMPARISON_ALWAYS,
                BorderColor: [0.0; 4],
                MinLOD: 0.0,
                MaxLOD: D3D11_FLOAT32_MAX,
            };
            let mut output = None;
            device.CreateSamplerState(&desc, Some(&mut output))?;
            [output]
        };

        Ok(Self {
            global_params_buffer,
            sampler,
        })
    }
}

#[derive(Debug, Default)]
#[repr(C)]
struct GlobalParams {
    gamma_ratios: [f32; 4],
    viewport_size: [f32; 2],
    grayscale_enhanced_contrast: f32,
    _pad: u32,
}

struct PipelineState<T> {
    label: &'static str,
    vertex: ID3D11VertexShader,
    fragment: ID3D11PixelShader,
    buffer: ID3D11Buffer,
    buffer_size: usize,
    view: [Option<ID3D11ShaderResourceView>; 1],
    blend_state: ID3D11BlendState,
    _marker: std::marker::PhantomData<T>,
}

impl<T> PipelineState<T> {
    fn new(
        device: &ID3D11Device,
        label: &'static str,
        shader_module: ShaderModule,
        buffer_size: usize,
        blend_state: ID3D11BlendState,
    ) -> Result<Self> {
        let vertex = {
            let raw_shader = RawShaderBytes::new(shader_module, ShaderTarget::Vertex)?;
            create_vertex_shader(device, raw_shader.as_bytes())?
        };
        let fragment = {
            let raw_shader = RawShaderBytes::new(shader_module, ShaderTarget::Fragment)?;
            create_fragment_shader(device, raw_shader.as_bytes())?
        };
        let buffer = create_buffer(device, std::mem::size_of::<T>(), buffer_size)?;
        let view = create_buffer_view(device, &buffer)?;

        Ok(PipelineState {
            label,
            vertex,
            fragment,
            buffer,
            buffer_size,
            view,
            blend_state,
            _marker: std::marker::PhantomData,
        })
    }

    fn update_buffer(
        &mut self,
        device: &ID3D11Device,
        device_context: &ID3D11DeviceContext,
        data: &[T],
        last_pipeline: &mut Option<*const ()>,
    ) -> Result<()> {
        if self.buffer_size < data.len() {
            let new_buffer_size = std::cmp::max(data.len() * 3 / 2, self.buffer_size);
            log::info!(
                "Updating {} buffer size from {} to {}",
                self.label,
                self.buffer_size,
                new_buffer_size
            );
            let buffer = create_buffer(device, std::mem::size_of::<T>(), new_buffer_size)?;
            let view = create_buffer_view(device, &buffer)?;
            self.buffer = buffer;
            self.view = view;
            self.buffer_size = new_buffer_size;
            // Buffer view changed, invalidate pipeline state cache
            *last_pipeline = None;
        }
        update_buffer(device_context, &self.buffer, data)
    }

    fn draw(
        &self,
        device_context: &ID3D11DeviceContext,
        viewport: &[D3D11_VIEWPORT],
        global_params: &[Option<ID3D11Buffer>],
        topology: D3D_PRIMITIVE_TOPOLOGY,
        vertex_count: u32,
        instance_count: u32,
        last_pipeline: &mut Option<*const ()>,
    ) -> Result<()> {
        let self_ptr = self as *const Self as *const ();
        if *last_pipeline != Some(self_ptr) {
            set_pipeline_state(
                device_context,
                &self.view,
                topology,
                viewport,
                &self.vertex,
                &self.fragment,
                global_params,
                &self.blend_state,
            );
            *last_pipeline = Some(self_ptr);
        }
        unsafe {
            device_context.DrawInstanced(vertex_count, instance_count, 0, 0);
        }
        Ok(())
    }

    fn draw_with_texture(
        &self,
        device_context: &ID3D11DeviceContext,
        texture: &[Option<ID3D11ShaderResourceView>],
        viewport: &[D3D11_VIEWPORT],
        global_params: &[Option<ID3D11Buffer>],
        sampler: &[Option<ID3D11SamplerState>],
        instance_count: u32,
        last_pipeline: &mut Option<*const ()>,
    ) -> Result<()> {
        let self_ptr = self as *const Self as *const ();
        if *last_pipeline != Some(self_ptr) {
            set_pipeline_state(
                device_context,
                &self.view,
                D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
                viewport,
                &self.vertex,
                &self.fragment,
                global_params,
                &self.blend_state,
            );
            *last_pipeline = Some(self_ptr);
        }
        unsafe {
            // Always set texture SRV — it changes per batch for different atlas textures
            device_context.PSSetSamplers(0, Some(sampler));
            device_context.VSSetShaderResources(0, Some(texture));
            device_context.PSSetShaderResources(0, Some(texture));

            device_context.DrawInstanced(4, instance_count, 0, 0);
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
struct PathRasterizationSprite {
    xy_position: Point<ScaledPixels>,
    st_position: Point<f32>,
    color: Background,
    bounds: Bounds<ScaledPixels>,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct PathSprite {
    bounds: Bounds<ScaledPixels>,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct BlurPass {
    target_bounds: Bounds<ScaledPixels>,
    sample_bounds: Bounds<ScaledPixels>,
    clip_bounds: Bounds<ScaledPixels>,
    corner_radii: Corners<ScaledPixels>,
    tint: Hsla,
    blur_radius: ScaledPixels,
    saturation: f32,
}

impl BlurPass {
    fn horizontal(blur_rect: &BlurRect, capture_bounds: Bounds<ScaledPixels>) -> Self {
        Self {
            target_bounds: capture_bounds,
            sample_bounds: capture_bounds,
            clip_bounds: capture_bounds,
            corner_radii: Corners::default(),
            tint: Hsla::transparent_black(),
            blur_radius: blur_rect.blur_radius,
            saturation: 1.0,
        }
    }

    fn composite(blur_rect: &BlurRect, capture_bounds: Bounds<ScaledPixels>) -> Self {
        Self {
            target_bounds: blur_rect.bounds,
            sample_bounds: capture_bounds,
            clip_bounds: blur_rect.content_mask.bounds,
            corner_radii: blur_rect.corner_radii,
            tint: blur_rect.tint,
            blur_radius: blur_rect.blur_radius,
            saturation: blur_rect.saturation,
        }
    }
}

impl Drop for DirectXRenderer {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        report_live_objects(&self.devices.device).ok();
        unsafe {
            ManuallyDrop::drop(&mut self.devices);
            ManuallyDrop::drop(&mut self.resources);
        }
    }
}

#[inline]
fn get_comp_device(dxgi_device: &IDXGIDevice) -> Result<IDCompositionDevice> {
    Ok(unsafe { DCompositionCreateDevice(dxgi_device)? })
}

fn create_swap_chain_for_composition(
    dxgi_factory: &IDXGIFactory6,
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> Result<IDXGISwapChain1> {
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: width,
        Height: height,
        Format: RENDER_TARGET_FORMAT,
        Stereo: false.into(),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: BUFFER_COUNT as u32,
        // Composition SwapChains only support the DXGI_SCALING_STRETCH Scaling.
        Scaling: DXGI_SCALING_STRETCH,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
        AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
        Flags: 0,
    };
    Ok(unsafe { dxgi_factory.CreateSwapChainForComposition(device, &desc, None)? })
}

fn create_swap_chain(
    dxgi_factory: &IDXGIFactory6,
    device: &ID3D11Device,
    hwnd: HWND,
    width: u32,
    height: u32,
) -> Result<IDXGISwapChain1> {
    use windows::Win32::Graphics::Dxgi::DXGI_MWA_NO_ALT_ENTER;

    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: width,
        Height: height,
        Format: RENDER_TARGET_FORMAT,
        Stereo: false.into(),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: BUFFER_COUNT as u32,
        Scaling: DXGI_SCALING_NONE,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
        AlphaMode: DXGI_ALPHA_MODE_IGNORE,
        Flags: 0,
    };
    let swap_chain =
        unsafe { dxgi_factory.CreateSwapChainForHwnd(device, hwnd, &desc, None, None) }?;
    unsafe { dxgi_factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER) }?;
    Ok(swap_chain)
}

#[inline]
fn create_resources(
    devices: &DirectXRendererDevices,
    swap_chain: &IDXGISwapChain1,
    width: u32,
    height: u32,
) -> Result<(
    ID3D11Texture2D,
    [Option<ID3D11RenderTargetView>; 1],
    ID3D11Texture2D,
    [Option<ID3D11ShaderResourceView>; 1],
    ID3D11Texture2D,
    [Option<ID3D11RenderTargetView>; 1],
    ID3D11Texture2D,
    [Option<ID3D11RenderTargetView>; 1],
    ID3D11Texture2D,
    [Option<ID3D11ShaderResourceView>; 1],
    ID3D11Texture2D,
    [Option<ID3D11ShaderResourceView>; 1],
    [Option<ID3D11RenderTargetView>; 1],
    [D3D11_VIEWPORT; 1],
)> {
    let (render_target, render_target_view) =
        create_render_target_and_its_view(swap_chain, &devices.device)?;
    let (path_intermediate_texture, path_intermediate_srv) =
        create_path_intermediate_texture(&devices.device, width, height)?;
    let (path_intermediate_msaa_texture, path_intermediate_msaa_view) =
        create_path_intermediate_msaa_texture_and_view(&devices.device, width, height)?;
    let (cached_surface_texture, cached_surface_view) =
        create_cached_surface_texture_and_view(&devices.device, width, height)?;
    let (blur_source_texture, blur_source_srv, _) =
        create_blur_intermediate_texture_and_views(&devices.device, width, height)?;
    let (blur_horizontal_texture, blur_horizontal_srv, blur_horizontal_view) =
        create_blur_intermediate_texture_and_views(&devices.device, width, height)?;
    let viewport = set_viewport(&devices.device_context, width as f32, height as f32);
    Ok((
        render_target,
        render_target_view,
        path_intermediate_texture,
        path_intermediate_srv,
        path_intermediate_msaa_texture,
        path_intermediate_msaa_view,
        cached_surface_texture,
        cached_surface_view,
        blur_source_texture,
        blur_source_srv,
        blur_horizontal_texture,
        blur_horizontal_srv,
        blur_horizontal_view,
        viewport,
    ))
}

#[inline]
fn create_blur_intermediate_texture_and_views(
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> Result<(
    ID3D11Texture2D,
    [Option<ID3D11ShaderResourceView>; 1],
    [Option<ID3D11RenderTargetView>; 1],
)> {
    let texture = unsafe {
        let mut output = None;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: RENDER_TARGET_FORMAT,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        device.CreateTexture2D(&desc, None, Some(&mut output))?;
        require_com_output(output, "CreateTexture2D for blur intermediate")?
    };

    let mut shader_resource_view = None;
    let mut render_target_view = None;
    unsafe {
        device.CreateShaderResourceView(&texture, None, Some(&mut shader_resource_view))?;
        device.CreateRenderTargetView(&texture, None, Some(&mut render_target_view))?;
    }

    Ok((
        texture,
        [Some(require_com_output(
            shader_resource_view,
            "CreateShaderResourceView for blur intermediate",
        )?)],
        [Some(require_com_output(
            render_target_view,
            "CreateRenderTargetView for blur intermediate",
        )?)],
    ))
}

#[inline]
fn create_cached_surface_texture_and_view(
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> Result<(ID3D11Texture2D, [Option<ID3D11RenderTargetView>; 1])> {
    let texture = unsafe {
        let mut output = None;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: RENDER_TARGET_FORMAT,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        device.CreateTexture2D(&desc, None, Some(&mut output))?;
        require_com_output(output, "CreateTexture2D for cached surface")?
    };

    let mut render_target_view = None;
    unsafe { device.CreateRenderTargetView(&texture, None, Some(&mut render_target_view))? };
    Ok((
        texture,
        [Some(require_com_output(
            render_target_view,
            "CreateRenderTargetView for cached surface",
        )?)],
    ))
}

#[inline]
fn create_render_target_and_its_view(
    swap_chain: &IDXGISwapChain1,
    device: &ID3D11Device,
) -> Result<(ID3D11Texture2D, [Option<ID3D11RenderTargetView>; 1])> {
    let render_target: ID3D11Texture2D = unsafe { swap_chain.GetBuffer(0) }?;
    let mut render_target_view = None;
    unsafe { device.CreateRenderTargetView(&render_target, None, Some(&mut render_target_view))? };
    Ok((
        render_target,
        [Some(require_com_output(
            render_target_view,
            "CreateRenderTargetView for swap chain",
        )?)],
    ))
}

#[inline]
fn create_path_intermediate_texture(
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> Result<(ID3D11Texture2D, [Option<ID3D11ShaderResourceView>; 1])> {
    let texture = unsafe {
        let mut output = None;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: RENDER_TARGET_FORMAT,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        device.CreateTexture2D(&desc, None, Some(&mut output))?;
        require_com_output(output, "CreateTexture2D for path intermediate")?
    };

    let mut shader_resource_view = None;
    unsafe { device.CreateShaderResourceView(&texture, None, Some(&mut shader_resource_view))? };

    Ok((
        texture,
        [Some(require_com_output(
            shader_resource_view,
            "CreateShaderResourceView for path intermediate",
        )?)],
    ))
}

#[inline]
fn create_path_intermediate_msaa_texture_and_view(
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> Result<(ID3D11Texture2D, [Option<ID3D11RenderTargetView>; 1])> {
    let msaa_texture = unsafe {
        let mut output = None;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: RENDER_TARGET_FORMAT,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: PATH_MULTISAMPLE_COUNT,
                Quality: D3D11_STANDARD_MULTISAMPLE_PATTERN.0 as u32,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        device.CreateTexture2D(&desc, None, Some(&mut output))?;
        require_com_output(output, "CreateTexture2D for path MSAA intermediate")?
    };
    let mut msaa_view = None;
    unsafe { device.CreateRenderTargetView(&msaa_texture, None, Some(&mut msaa_view))? };
    Ok((
        msaa_texture,
        [Some(require_com_output(
            msaa_view,
            "CreateRenderTargetView for path MSAA intermediate",
        )?)],
    ))
}

#[inline]
fn set_viewport(
    device_context: &ID3D11DeviceContext,
    width: f32,
    height: f32,
) -> [D3D11_VIEWPORT; 1] {
    let viewport = [D3D11_VIEWPORT {
        TopLeftX: 0.0,
        TopLeftY: 0.0,
        Width: width,
        Height: height,
        MinDepth: 0.0,
        MaxDepth: 1.0,
    }];
    unsafe { device_context.RSSetViewports(Some(&viewport)) };
    viewport
}

#[inline]
fn set_rasterizer_state(device: &ID3D11Device, device_context: &ID3D11DeviceContext) -> Result<()> {
    let desc = D3D11_RASTERIZER_DESC {
        FillMode: D3D11_FILL_SOLID,
        CullMode: D3D11_CULL_NONE,
        FrontCounterClockwise: false.into(),
        DepthBias: 0,
        DepthBiasClamp: 0.0,
        SlopeScaledDepthBias: 0.0,
        DepthClipEnable: true.into(),
        ScissorEnable: false.into(),
        MultisampleEnable: true.into(),
        AntialiasedLineEnable: false.into(),
    };
    let rasterizer_state = unsafe {
        let mut state = None;
        device.CreateRasterizerState(&desc, Some(&mut state))?;
        require_com_output(state, "CreateRasterizerState")?
    };
    unsafe { device_context.RSSetState(&rasterizer_state) };
    Ok(())
}

// https://learn.microsoft.com/en-us/windows/win32/api/d3d11/ns-d3d11-d3d11_blend_desc
#[inline]
fn create_blend_state(device: &ID3D11Device) -> Result<ID3D11BlendState> {
    // If the feature level is set to greater than D3D_FEATURE_LEVEL_9_3, the display
    // device performs the blend in linear space, which is ideal.
    let mut desc = D3D11_BLEND_DESC::default();
    desc.RenderTarget[0].BlendEnable = true.into();
    desc.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].SrcBlend = D3D11_BLEND_SRC_ALPHA;
    desc.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
    desc.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
    desc.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_ONE;
    desc.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
    unsafe {
        let mut state = None;
        device.CreateBlendState(&desc, Some(&mut state))?;
        require_com_output(state, "CreateBlendState")
    }
}

#[inline]
fn create_blend_state_for_path_rasterization(device: &ID3D11Device) -> Result<ID3D11BlendState> {
    // If the feature level is set to greater than D3D_FEATURE_LEVEL_9_3, the display
    // device performs the blend in linear space, which is ideal.
    let mut desc = D3D11_BLEND_DESC::default();
    desc.RenderTarget[0].BlendEnable = true.into();
    desc.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].SrcBlend = D3D11_BLEND_ONE;
    desc.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
    desc.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
    desc.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
    desc.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
    unsafe {
        let mut state = None;
        device.CreateBlendState(&desc, Some(&mut state))?;
        require_com_output(state, "CreateBlendState for path rasterization")
    }
}

#[inline]
fn create_blend_state_for_path_sprite(device: &ID3D11Device) -> Result<ID3D11BlendState> {
    // If the feature level is set to greater than D3D_FEATURE_LEVEL_9_3, the display
    // device performs the blend in linear space, which is ideal.
    let mut desc = D3D11_BLEND_DESC::default();
    desc.RenderTarget[0].BlendEnable = true.into();
    desc.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].SrcBlend = D3D11_BLEND_ONE;
    desc.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
    desc.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
    desc.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_ONE;
    desc.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
    unsafe {
        let mut state = None;
        device.CreateBlendState(&desc, Some(&mut state))?;
        require_com_output(state, "CreateBlendState for path sprite")
    }
}

#[inline]
fn create_vertex_shader(device: &ID3D11Device, bytes: &[u8]) -> Result<ID3D11VertexShader> {
    unsafe {
        let mut shader = None;
        device.CreateVertexShader(bytes, None, Some(&mut shader))?;
        require_com_output(shader, "CreateVertexShader")
    }
}

#[inline]
fn create_fragment_shader(device: &ID3D11Device, bytes: &[u8]) -> Result<ID3D11PixelShader> {
    unsafe {
        let mut shader = None;
        device.CreatePixelShader(bytes, None, Some(&mut shader))?;
        require_com_output(shader, "CreatePixelShader")
    }
}

#[inline]
fn create_buffer(
    device: &ID3D11Device,
    element_size: usize,
    buffer_size: usize,
) -> Result<ID3D11Buffer> {
    let byte_width = element_size
        .checked_mul(buffer_size)
        .and_then(|bytes| u32::try_from(bytes).ok())
        .filter(|bytes| *bytes > 0)
        .ok_or_else(|| anyhow::anyhow!("DirectX buffer size is zero or exceeds u32"))?;
    let element_size = u32::try_from(element_size)
        .ok()
        .filter(|size| *size > 0)
        .ok_or_else(|| anyhow::anyhow!("DirectX buffer element size is zero or exceeds u32"))?;
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: byte_width,
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        MiscFlags: D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32,
        StructureByteStride: element_size,
    };
    let mut buffer = None;
    unsafe { device.CreateBuffer(&desc, None, Some(&mut buffer)) }?;
    require_com_output(buffer, "CreateBuffer")
}

#[inline]
fn create_buffer_view(
    device: &ID3D11Device,
    buffer: &ID3D11Buffer,
) -> Result<[Option<ID3D11ShaderResourceView>; 1]> {
    let mut view = None;
    unsafe { device.CreateShaderResourceView(buffer, None, Some(&mut view)) }?;
    Ok([Some(require_com_output(
        view,
        "CreateShaderResourceView for structured buffer",
    )?)])
}

#[inline]
fn update_buffer<T>(
    device_context: &ID3D11DeviceContext,
    buffer: &ID3D11Buffer,
    data: &[T],
) -> Result<()> {
    unsafe {
        let mut dest = std::mem::zeroed();
        device_context.Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut dest))?;
        std::ptr::copy_nonoverlapping(data.as_ptr(), dest.pData as _, data.len());
        device_context.Unmap(buffer, 0);
    }
    Ok(())
}

#[inline]
fn set_pipeline_state(
    device_context: &ID3D11DeviceContext,
    buffer_view: &[Option<ID3D11ShaderResourceView>],
    topology: D3D_PRIMITIVE_TOPOLOGY,
    viewport: &[D3D11_VIEWPORT],
    vertex_shader: &ID3D11VertexShader,
    fragment_shader: &ID3D11PixelShader,
    global_params: &[Option<ID3D11Buffer>],
    blend_state: &ID3D11BlendState,
) {
    unsafe {
        device_context.VSSetShaderResources(1, Some(buffer_view));
        device_context.PSSetShaderResources(1, Some(buffer_view));
        device_context.IASetPrimitiveTopology(topology);
        device_context.RSSetViewports(Some(viewport));
        device_context.VSSetShader(vertex_shader, None);
        device_context.PSSetShader(fragment_shader, None);
        device_context.VSSetConstantBuffers(0, Some(global_params));
        device_context.PSSetConstantBuffers(0, Some(global_params));
        device_context.OMSetBlendState(blend_state, None, 0xFFFFFFFF);
    }
}

#[cfg(debug_assertions)]
fn report_live_objects(device: &ID3D11Device) -> Result<()> {
    let debug_device: ID3D11Debug = device.cast()?;
    unsafe {
        debug_device.ReportLiveDeviceObjects(D3D11_RLDO_DETAIL)?;
    }
    Ok(())
}

const BUFFER_COUNT: usize = 3;

pub(crate) mod shader_resources {
    use anyhow::Result;

    #[cfg(debug_assertions)]
    use windows::{
        Win32::Graphics::Direct3D::{
            Fxc::{D3DCOMPILE_DEBUG, D3DCOMPILE_SKIP_OPTIMIZATION, D3DCompileFromFile},
            ID3DBlob,
        },
        core::{HSTRING, PCSTR},
    };

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) enum ShaderModule {
        Quad,
        BlurHorizontal,
        BlurComposite,
        Shadow,
        Underline,
        PathRasterization,
        PathSprite,
        MonochromeSprite,
        PolychromeSprite,
        EmojiRasterization,
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) enum ShaderTarget {
        Vertex,
        Fragment,
    }

    pub(crate) struct RawShaderBytes<'t> {
        inner: &'t [u8],

        #[cfg(debug_assertions)]
        _blob: ID3DBlob,
    }

    impl<'t> RawShaderBytes<'t> {
        pub(crate) fn new(module: ShaderModule, target: ShaderTarget) -> Result<Self> {
            #[cfg(not(debug_assertions))]
            {
                Ok(Self::from_bytes(module, target))
            }
            #[cfg(debug_assertions)]
            {
                let blob = build_shader_blob(module, target)?;
                let buffer_len = unsafe { blob.GetBufferSize() };
                let buffer_ptr = unsafe { blob.GetBufferPointer() };
                anyhow::ensure!(
                    buffer_len > 0 && buffer_len <= 64 * 1024 * 1024,
                    "compiled shader blob has an invalid size"
                );
                anyhow::ensure!(!buffer_ptr.is_null(), "compiled shader blob has no data");
                let inner =
                    unsafe { std::slice::from_raw_parts(buffer_ptr.cast::<u8>(), buffer_len) };
                Ok(Self { inner, _blob: blob })
            }
        }

        pub(crate) fn as_bytes(&'t self) -> &'t [u8] {
            self.inner
        }

        #[cfg(not(debug_assertions))]
        fn from_bytes(module: ShaderModule, target: ShaderTarget) -> Self {
            let bytes = match module {
                ShaderModule::Quad => match target {
                    ShaderTarget::Vertex => QUAD_VERTEX_BYTES,
                    ShaderTarget::Fragment => QUAD_FRAGMENT_BYTES,
                },
                ShaderModule::BlurHorizontal => match target {
                    ShaderTarget::Vertex => BLUR_HORIZONTAL_VERTEX_BYTES,
                    ShaderTarget::Fragment => BLUR_HORIZONTAL_FRAGMENT_BYTES,
                },
                ShaderModule::BlurComposite => match target {
                    ShaderTarget::Vertex => BLUR_COMPOSITE_VERTEX_BYTES,
                    ShaderTarget::Fragment => BLUR_COMPOSITE_FRAGMENT_BYTES,
                },
                ShaderModule::Shadow => match target {
                    ShaderTarget::Vertex => SHADOW_VERTEX_BYTES,
                    ShaderTarget::Fragment => SHADOW_FRAGMENT_BYTES,
                },
                ShaderModule::Underline => match target {
                    ShaderTarget::Vertex => UNDERLINE_VERTEX_BYTES,
                    ShaderTarget::Fragment => UNDERLINE_FRAGMENT_BYTES,
                },
                ShaderModule::PathRasterization => match target {
                    ShaderTarget::Vertex => PATH_RASTERIZATION_VERTEX_BYTES,
                    ShaderTarget::Fragment => PATH_RASTERIZATION_FRAGMENT_BYTES,
                },
                ShaderModule::PathSprite => match target {
                    ShaderTarget::Vertex => PATH_SPRITE_VERTEX_BYTES,
                    ShaderTarget::Fragment => PATH_SPRITE_FRAGMENT_BYTES,
                },
                ShaderModule::MonochromeSprite => match target {
                    ShaderTarget::Vertex => MONOCHROME_SPRITE_VERTEX_BYTES,
                    ShaderTarget::Fragment => MONOCHROME_SPRITE_FRAGMENT_BYTES,
                },
                ShaderModule::PolychromeSprite => match target {
                    ShaderTarget::Vertex => POLYCHROME_SPRITE_VERTEX_BYTES,
                    ShaderTarget::Fragment => POLYCHROME_SPRITE_FRAGMENT_BYTES,
                },
                ShaderModule::EmojiRasterization => match target {
                    ShaderTarget::Vertex => EMOJI_RASTERIZATION_VERTEX_BYTES,
                    ShaderTarget::Fragment => EMOJI_RASTERIZATION_FRAGMENT_BYTES,
                },
            };
            Self { inner: bytes }
        }
    }

    #[cfg(debug_assertions)]
    pub(super) fn build_shader_blob(entry: ShaderModule, target: ShaderTarget) -> Result<ID3DBlob> {
        unsafe {
            use windows::Win32::Graphics::{
                Direct3D::ID3DInclude, Hlsl::D3D_COMPILE_STANDARD_FILE_INCLUDE,
            };

            let shader_name = if matches!(entry, ShaderModule::EmojiRasterization) {
                "color_text_raster.hlsl"
            } else {
                "shaders.hlsl"
            };

            let entry = format!(
                "{}_{}\0",
                entry.as_str(),
                match target {
                    ShaderTarget::Vertex => "vertex",
                    ShaderTarget::Fragment => "fragment",
                }
            );
            let target = match target {
                ShaderTarget::Vertex => "vs_4_1\0",
                ShaderTarget::Fragment => "ps_4_1\0",
            };

            let mut compile_blob = None;
            let mut error_blob = None;
            let shader_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join(&format!("src/platform/windows/{}", shader_name))
                .canonicalize()?;

            let entry_point = PCSTR::from_raw(entry.as_ptr());
            let target_cstr = PCSTR::from_raw(target.as_ptr());

            // really dirty trick because winapi bindings are unhappy otherwise
            let include_handler = &std::mem::transmute::<usize, ID3DInclude>(
                D3D_COMPILE_STANDARD_FILE_INCLUDE as usize,
            );

            let ret = D3DCompileFromFile(
                &HSTRING::from(shader_path.as_os_str()),
                None,
                include_handler,
                entry_point,
                target_cstr,
                D3DCOMPILE_DEBUG | D3DCOMPILE_SKIP_OPTIMIZATION,
                0,
                &mut compile_blob,
                Some(&mut error_blob),
            );
            if ret.is_err() {
                let Some(error_blob) = error_blob else {
                    return Err(anyhow::anyhow!("{ret:?}"));
                };
                let error_len = error_blob.GetBufferSize().min(1024 * 1024);
                let error_ptr = error_blob.GetBufferPointer().cast::<u8>();
                if error_ptr.is_null() || error_len == 0 {
                    return Err(anyhow::anyhow!(
                        "shader compilation failed without diagnostics"
                    ));
                }
                let error_bytes = std::slice::from_raw_parts(error_ptr, error_len);
                let error_end = error_bytes
                    .iter()
                    .position(|byte| *byte == 0)
                    .unwrap_or(error_bytes.len());
                let error_string = String::from_utf8_lossy(&error_bytes[..error_end]);
                log::error!("Shader compile error: {}", error_string);
                return Err(anyhow::anyhow!("Compile error: {}", error_string));
            }
            super::require_com_output(compile_blob, "D3DCompileFromFile")
        }
    }

    #[cfg(not(debug_assertions))]
    include!(concat!(env!("OUT_DIR"), "/shaders_bytes.rs"));

    #[cfg(debug_assertions)]
    impl ShaderModule {
        pub fn as_str(&self) -> &str {
            match self {
                ShaderModule::Quad => "quad",
                ShaderModule::BlurHorizontal => "blur_horizontal",
                ShaderModule::BlurComposite => "blur_composite",
                ShaderModule::Shadow => "shadow",
                ShaderModule::Underline => "underline",
                ShaderModule::PathRasterization => "path_rasterization",
                ShaderModule::PathSprite => "path_sprite",
                ShaderModule::MonochromeSprite => "monochrome_sprite",
                ShaderModule::PolychromeSprite => "polychrome_sprite",
                ShaderModule::EmojiRasterization => "emoji_rasterization",
            }
        }
    }
}

mod nvidia {
    use std::os::raw::{c_char, c_int, c_uint};

    use anyhow::Result;
    use windows::{Win32::System::LibraryLoader::GetProcAddress, core::s};

    use crate::with_dll_library;

    // https://github.com/NVIDIA/nvapi/blob/7cb76fce2f52de818b3da497af646af1ec16ce27/nvapi_lite_common.h#L180
    const NVAPI_SHORT_STRING_MAX: usize = 64;

    // https://github.com/NVIDIA/nvapi/blob/7cb76fce2f52de818b3da497af646af1ec16ce27/nvapi_lite_common.h#L235
    #[allow(non_camel_case_types)]
    type NvAPI_ShortString = [c_char; NVAPI_SHORT_STRING_MAX];

    // https://github.com/NVIDIA/nvapi/blob/7cb76fce2f52de818b3da497af646af1ec16ce27/nvapi_lite_common.h#L447
    #[allow(non_camel_case_types)]
    type NvAPI_SYS_GetDriverAndBranchVersion_t = unsafe extern "C" fn(
        driver_version: *mut c_uint,
        build_branch_string: *mut NvAPI_ShortString,
    ) -> c_int;

    pub(super) fn get_driver_version() -> Result<String> {
        #[cfg(target_pointer_width = "64")]
        let nvidia_dll_name = s!("nvapi64.dll");
        #[cfg(target_pointer_width = "32")]
        let nvidia_dll_name = s!("nvapi.dll");

        with_dll_library(nvidia_dll_name, |nvidia_dll| unsafe {
            let nvapi_query_addr = GetProcAddress(nvidia_dll, s!("nvapi_QueryInterface"))
                .ok_or_else(|| anyhow::anyhow!("Failed to get nvapi_QueryInterface address"))?;
            let nvapi_query: extern "C" fn(u32) -> *mut () = std::mem::transmute(nvapi_query_addr);

            // https://github.com/NVIDIA/nvapi/blob/7cb76fce2f52de818b3da497af646af1ec16ce27/nvapi_interface.h#L41
            let nvapi_get_driver_version_ptr = nvapi_query(0x2926aaad);
            if nvapi_get_driver_version_ptr.is_null() {
                anyhow::bail!("Failed to get NVIDIA driver version function pointer");
            }
            let nvapi_get_driver_version: NvAPI_SYS_GetDriverAndBranchVersion_t =
                std::mem::transmute(nvapi_get_driver_version_ptr);

            let mut driver_version: c_uint = 0;
            let mut build_branch_string: NvAPI_ShortString = [0; NVAPI_SHORT_STRING_MAX];
            let result = nvapi_get_driver_version(
                &mut driver_version as *mut c_uint,
                &mut build_branch_string as *mut NvAPI_ShortString,
            );

            if result != 0 {
                anyhow::bail!(
                    "Failed to get NVIDIA driver version, error code: {}",
                    result
                );
            }
            let major = driver_version / 100;
            let minor = driver_version % 100;
            let branch_end = build_branch_string
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(build_branch_string.len());
            let branch_bytes =
                std::slice::from_raw_parts(build_branch_string.as_ptr().cast::<u8>(), branch_end);
            let branch_string = String::from_utf8_lossy(branch_bytes);
            Ok(format!("{}.{} {}", major, minor, branch_string))
        })
    }
}

mod amd {
    use std::os::raw::{c_char, c_int, c_void};

    use anyhow::Result;
    use windows::{Win32::System::LibraryLoader::GetProcAddress, core::s};

    use crate::with_dll_library;

    // https://github.com/GPUOpen-LibrariesAndSDKs/AGS_SDK/blob/5d8812d703d0335741b6f7ffc37838eeb8b967f7/ags_lib/inc/amd_ags.h#L145
    const AGS_CURRENT_VERSION: i32 = (6 << 22) | (3 << 12);

    // https://github.com/GPUOpen-LibrariesAndSDKs/AGS_SDK/blob/5d8812d703d0335741b6f7ffc37838eeb8b967f7/ags_lib/inc/amd_ags.h#L204
    // This is an opaque type, using struct to represent it properly for FFI
    #[repr(C)]
    struct AGSContext {
        _private: [u8; 0],
    }

    #[repr(C)]
    pub struct AGSGPUInfo {
        pub driver_version: *const c_char,
        pub radeon_software_version: *const c_char,
        pub num_devices: c_int,
        pub devices: *mut c_void,
    }

    // https://github.com/GPUOpen-LibrariesAndSDKs/AGS_SDK/blob/5d8812d703d0335741b6f7ffc37838eeb8b967f7/ags_lib/inc/amd_ags.h#L429
    #[allow(non_camel_case_types)]
    type agsInitialize_t = unsafe extern "C" fn(
        version: c_int,
        config: *const c_void,
        context: *mut *mut AGSContext,
        gpu_info: *mut AGSGPUInfo,
    ) -> c_int;

    // https://github.com/GPUOpen-LibrariesAndSDKs/AGS_SDK/blob/5d8812d703d0335741b6f7ffc37838eeb8b967f7/ags_lib/inc/amd_ags.h#L436
    #[allow(non_camel_case_types)]
    type agsDeInitialize_t = unsafe extern "C" fn(context: *mut AGSContext) -> c_int;

    struct AgsContextGuard {
        context: *mut AGSContext,
        deinitialize: agsDeInitialize_t,
    }

    impl Drop for AgsContextGuard {
        fn drop(&mut self) {
            unsafe {
                (self.deinitialize)(self.context);
            }
        }
    }

    unsafe fn bounded_driver_string(value: *const c_char) -> Option<String> {
        if value.is_null() {
            return None;
        }
        const MAX_DRIVER_STRING_BYTES: usize = 4_096;
        let len = unsafe { libc::strnlen(value, MAX_DRIVER_STRING_BYTES + 1) };
        if len > MAX_DRIVER_STRING_BYTES {
            return None;
        }
        let bytes = unsafe { std::slice::from_raw_parts(value.cast::<u8>(), len) };
        Some(String::from_utf8_lossy(bytes).into_owned())
    }

    pub(super) fn get_driver_version() -> Result<String> {
        #[cfg(target_pointer_width = "64")]
        let amd_dll_name = s!("amd_ags_x64.dll");
        #[cfg(target_pointer_width = "32")]
        let amd_dll_name = s!("amd_ags_x86.dll");

        with_dll_library(amd_dll_name, |amd_dll| unsafe {
            let ags_initialize_addr = GetProcAddress(amd_dll, s!("agsInitialize"))
                .ok_or_else(|| anyhow::anyhow!("Failed to get agsInitialize address"))?;
            let ags_deinitialize_addr = GetProcAddress(amd_dll, s!("agsDeInitialize"))
                .ok_or_else(|| anyhow::anyhow!("Failed to get agsDeInitialize address"))?;

            let ags_initialize: agsInitialize_t = std::mem::transmute(ags_initialize_addr);
            let ags_deinitialize: agsDeInitialize_t = std::mem::transmute(ags_deinitialize_addr);

            let mut context: *mut AGSContext = std::ptr::null_mut();
            let mut gpu_info: AGSGPUInfo = AGSGPUInfo {
                driver_version: std::ptr::null(),
                radeon_software_version: std::ptr::null(),
                num_devices: 0,
                devices: std::ptr::null_mut(),
            };

            let result = ags_initialize(
                AGS_CURRENT_VERSION,
                std::ptr::null(),
                &mut context,
                &mut gpu_info,
            );
            if result != 0 {
                anyhow::bail!("Failed to initialize AMD AGS, error code: {}", result);
            }
            if context.is_null() {
                anyhow::bail!("AMD AGS initialized without returning a context");
            }
            let _context = AgsContextGuard {
                context,
                deinitialize: ags_deinitialize,
            };

            // Vulkan actually returns this as the driver version
            let software_version = bounded_driver_string(gpu_info.radeon_software_version)
                .unwrap_or_else(|| "Unknown Radeon Software Version".to_string());

            let driver_version = bounded_driver_string(gpu_info.driver_version)
                .unwrap_or_else(|| "Unknown Radeon Driver Version".to_string());

            Ok(format!("{} ({})", software_version, driver_version))
        })
    }
}

mod dxgi {
    use windows::{
        Win32::Graphics::Dxgi::{IDXGIAdapter1, IDXGIDevice},
        core::Interface,
    };

    pub(super) fn get_driver_version(adapter: &IDXGIAdapter1) -> anyhow::Result<String> {
        let number = unsafe { adapter.CheckInterfaceSupport(&IDXGIDevice::IID as _) }?;
        Ok(format!(
            "{}.{}.{}.{}",
            number >> 48,
            (number >> 32) & 0xFFFF,
            (number >> 16) & 0xFFFF,
            number & 0xFFFF
        ))
    }
}

//! Bounded real-window renderer and scene-readback smoke for release CI.

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::{
        collections::HashSet,
        fs,
        path::{Path, PathBuf},
        sync::{
            Arc,
            atomic::{AtomicU8, AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };

    use anyhow::{Context as _, ensure};
    use image::ImageFormat as DecodedImageFormat;
    use kael::prelude::*;
    use kael::{
        App, Application, Bounds, Context, ImageFormat, Render, Window, WindowBounds,
        WindowOptions, div, hsla, linear_color_stop, linear_gradient, px, relative, rgb, size,
    };

    const WIDTH: f32 = 720.0;
    const HEIGHT: f32 = 460.0;
    const REQUIRED_RENDER_REVISIONS: usize = 4;
    const SMOKE_TIMEOUT: Duration = Duration::from_secs(20);

    struct RendererSmoke {
        revision: usize,
        render_count: Arc<AtomicUsize>,
    }

    impl Render for RendererSmoke {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.render_count.fetch_add(1, Ordering::Release);
            let accent_hue = 0.54 + (self.revision % REQUIRED_RENDER_REVISIONS) as f32 * 0.035;
            let accent = hsla(accent_hue, 0.82, 0.58, 1.0);
            let secondary = hsla((accent_hue + 0.18) % 1.0, 0.76, 0.58, 1.0);

            div()
                .size_full()
                .flex()
                .flex_col()
                .bg(rgb(0x090f1f))
                .text_color(rgb(0xf6f8ff))
                .child(
                    div()
                        .h(px(96.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px(px(30.0))
                        .bg(linear_gradient(
                            104.0,
                            linear_color_stop(accent, 0.0),
                            linear_color_stop(secondary, 1.0),
                        ))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(4.0))
                                .child(div().text_size(px(28.0)).child("Kael native renderer"))
                                .child(
                                    div()
                                        .text_size(px(13.0))
                                        .text_color(hsla(0.0, 0.0, 1.0, 0.78))
                                        .child("retained scene · device-pixel readback"),
                                ),
                        )
                        .child(
                            div()
                                .px(px(14.0))
                                .py(px(8.0))
                                .rounded_full()
                                .border_1()
                                .border_color(hsla(0.0, 0.0, 1.0, 0.42))
                                .bg(hsla(0.0, 0.0, 0.0, 0.18))
                                .child(format!("frame revision {}", self.revision)),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .gap(px(18.0))
                        .p(px(24.0))
                        .child(
                            div()
                                .w(px(216.0))
                                .flex_none()
                                .flex()
                                .flex_col()
                                .gap(px(13.0))
                                .p(px(18.0))
                                .rounded(px(18.0))
                                .border_1()
                                .border_color(hsla(accent_hue, 0.55, 0.52, 0.44))
                                .bg(rgb(0x111a30))
                                .shadow_lg()
                                .child(div().text_size(px(16.0)).child("Scene batches"))
                                .children((0..5).map(|index| {
                                    let fraction = 0.38 + index as f32 * 0.115;
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(5.0))
                                        .child(
                                            div()
                                                .text_size(px(11.0))
                                                .text_color(rgb(0xaab6d6))
                                                .child(format!("retained layer {}", index + 1)),
                                        )
                                        .child(
                                            div()
                                                .h(px(9.0))
                                                .rounded_full()
                                                .bg(rgb(0x25304c))
                                                .child(
                                                    div()
                                                        .h_full()
                                                        .w(relative(fraction))
                                                        .rounded_full()
                                                        .bg(if index % 2 == 0 {
                                                            accent
                                                        } else {
                                                            secondary
                                                        }),
                                                ),
                                        )
                                })),
                        )
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(px(16.0))
                                .child(
                                    div()
                                        .h(px(146.0))
                                        .flex()
                                        .items_end()
                                        .gap(px(11.0))
                                        .p(px(18.0))
                                        .rounded(px(18.0))
                                        .border_1()
                                        .border_color(rgb(0x293653))
                                        .bg(linear_gradient(
                                            180.0,
                                            linear_color_stop(rgb(0x17223d), 0.0),
                                            linear_color_stop(rgb(0x0d1427), 1.0),
                                        ))
                                        .children((0..10).map(|index| {
                                            let height = 34.0
                                                + ((index * 37 + self.revision * 19) % 86) as f32;
                                            div()
                                                .flex_1()
                                                .h(px(height))
                                                .rounded_t(px(7.0))
                                                .bg(if index % 3 == 0 { secondary } else { accent })
                                        })),
                                )
                                .child(div().flex_1().grid().grid_cols(3).gap(px(12.0)).children(
                                    (0..6).map(|index| {
                                        let hue = (accent_hue + index as f32 * 0.095) % 1.0;
                                        div()
                                            .flex()
                                            .flex_col()
                                            .justify_between()
                                            .p(px(13.0))
                                            .rounded(px(14.0))
                                            .border_1()
                                            .border_color(hsla(hue, 0.56, 0.52, 0.36))
                                            .bg(hsla(hue, 0.45, 0.19, 1.0))
                                            .child(
                                                div()
                                                    .size(px(18.0))
                                                    .rounded_full()
                                                    .bg(hsla(hue, 0.82, 0.62, 1.0)),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.0))
                                                    .text_color(rgb(0xd7def4))
                                                    .child(format!("tile {:02}", index + 1)),
                                            )
                                    }),
                                )),
                        ),
                )
        }
    }

    struct VerifiedFrame {
        output: PathBuf,
        width: u32,
        height: u32,
        encoded_bytes: usize,
        distinct_colors: usize,
        text_probe_pixels: usize,
        checksum: u64,
        device_name: String,
        driver_name: String,
        driver_info: String,
        software_emulated: bool,
        scale_factor: f32,
    }

    fn backend_name() -> &'static str {
        #[cfg(target_os = "windows")]
        {
            "direct3d11"
        }
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        {
            "blade-vulkan"
        }
        #[cfg(all(target_os = "macos", feature = "macos-blade"))]
        {
            "blade-metal"
        }
        #[cfg(all(target_os = "macos", not(feature = "macos-blade")))]
        {
            "metal"
        }
    }

    fn default_png_path() -> PathBuf {
        PathBuf::from("target")
            .join("native-renderer-smoke")
            .join(format!("{}.png", std::env::consts::OS))
    }

    fn fnv1a64(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        })
    }

    fn verify_frame(window: &Window, output: &Path) -> anyhow::Result<VerifiedFrame> {
        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        let gpu = window
            .gpu_specs()
            .context("native renderer did not report a GPU adapter")?;
        #[cfg(target_os = "macos")]
        let gpu = window.gpu_specs().unwrap_or_else(|| kael::GpuSpecs {
            device_name: "Metal device (platform identity unavailable)".to_owned(),
            driver_name: "Metal".to_owned(),
            driver_info: "Kael macOS Metal backend".to_owned(),
            is_software_emulated: false,
        });

        ensure!(
            !gpu.device_name.trim().is_empty(),
            "native renderer reported an empty GPU device name"
        );
        if std::env::var_os("KAEL_EXPECT_SOFTWARE_RENDERER").is_some() {
            ensure!(
                gpu.is_software_emulated,
                "CI requested a software renderer but the adapter was reported as hardware"
            );
        }

        let viewport = window.viewport_size();
        let scale_factor = window.scale_factor();
        let expected_width = (f32::from(viewport.width) * scale_factor).round().max(1.0) as u32;
        let expected_height = (f32::from(viewport.height) * scale_factor).round().max(1.0) as u32;
        let image = window
            .export_frame_png()
            .context("export the retained renderer scene")?;
        ensure!(image.format() == ImageFormat::Png, "capture was not PNG");
        ensure!(
            image.bytes().starts_with(b"\x89PNG\r\n\x1a\n"),
            "capture is missing the PNG signature"
        );
        ensure!(image.byte_len() > 1_024, "capture is unexpectedly small");

        let decoded = image::load_from_memory_with_format(image.bytes(), DecodedImageFormat::Png)
            .context("decode exported PNG")?
            .to_rgba8();
        let (width, height) = decoded.dimensions();
        ensure!(
            width.abs_diff(expected_width) <= 2 && height.abs_diff(expected_height) <= 2,
            "device-pixel capture dimensions {width}x{height} do not match viewport {expected_width}x{expected_height} (scale {scale_factor})"
        );
        ensure!(
            width >= 320 && height >= 200,
            "capture dimensions are too small"
        );

        // Preserve the frame even when a later visual assertion fails. Hosted
        // GPU evidence must remain inspectable instead of reducing a rendering
        // regression to an opaque pixel-count message.
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create capture directory {}", parent.display()))?;
        }
        fs::write(output, image.bytes())
            .with_context(|| format!("write native renderer proof to {}", output.display()))?;

        let pixel_count = u64::from(width) * u64::from(height);
        let mut opaque_or_visible = 0_u64;
        let mut transparent = 0_u64;
        let mut partial_alpha = 0_u64;
        let mut opaque = 0_u64;
        let mut non_black_rgb = 0_u64;
        let mut minimum_luma = u16::MAX;
        let mut maximum_luma = 0_u16;
        let mut colors = HashSet::with_capacity(64);
        for pixel in decoded.pixels() {
            let [red, green, blue, alpha] = pixel.0;
            if alpha != 0 {
                opaque_or_visible += 1;
            }
            match alpha {
                0 => transparent += 1,
                255 => opaque += 1,
                _ => partial_alpha += 1,
            }
            if red != 0 || green != 0 || blue != 0 {
                non_black_rgb += 1;
            }
            let luma = (u16::from(red) * 54 + u16::from(green) * 183 + u16::from(blue) * 19) / 256;
            minimum_luma = minimum_luma.min(luma);
            maximum_luma = maximum_luma.max(luma);
            if colors.len() < 256 {
                colors.insert(u32::from_be_bytes([red, green, blue, alpha]));
            }
        }

        // The top-left header contains only a saturated blue-to-magenta
        // background plus neutral-white title/subtitle glyphs. Count neutral
        // light pixels in that bounded logical region so a missing font loader
        // or stale glyph atlas cannot pass as a shapes-only renderer proof.
        let probe_left = (20.0 * scale_factor).round().max(0.0) as u32;
        let probe_top = (8.0 * scale_factor).round().max(0.0) as u32;
        let probe_right = (520.0 * scale_factor).round().min(width as f32) as u32;
        let probe_bottom = (88.0 * scale_factor).round().min(height as f32) as u32;
        let mut text_probe_pixels = 0usize;
        for y in probe_top..probe_bottom {
            for x in probe_left..probe_right {
                let [red, green, blue, alpha] = decoded.get_pixel(x, y).0;
                let maximum = red.max(green).max(blue);
                let minimum = red.min(green).min(blue);
                if alpha >= 220 && minimum >= 180 && maximum.saturating_sub(minimum) <= 40 {
                    text_probe_pixels += 1;
                }
            }
        }
        ensure!(
            text_probe_pixels >= 128,
            "capture header contains only {text_probe_pixels} neutral light pixels; retained text/glyph-atlas rendering is missing"
        );
        ensure!(
            opaque_or_visible >= pixel_count * 9 / 10,
            "capture contains too few visible pixels ({opaque_or_visible}/{pixel_count}; alpha transparent={transparent}, partial={partial_alpha}, opaque={opaque}; non_black_rgb={non_black_rgb})"
        );
        ensure!(
            colors.len() >= 16,
            "capture has only {} distinct colors",
            colors.len()
        );
        ensure!(
            maximum_luma.saturating_sub(minimum_luma) >= 80,
            "capture luminance range is too narrow ({minimum_luma}..={maximum_luma})"
        );

        Ok(VerifiedFrame {
            output: output.to_path_buf(),
            width,
            height,
            encoded_bytes: image.byte_len(),
            distinct_colors: colors.len(),
            text_probe_pixels,
            checksum: fnv1a64(image.bytes()),
            device_name: gpu.device_name,
            driver_name: gpu.driver_name,
            driver_info: gpu.driver_info,
            software_emulated: gpu.is_software_emulated,
            scale_factor,
        })
    }

    pub fn run() -> anyhow::Result<()> {
        ensure!(
            std::env::var_os("KAEL_HEADLESS").is_none(),
            "KAEL_HEADLESS must be unset for the native renderer runtime proof"
        );
        let output = std::env::var_os("KAEL_NATIVE_RENDERER_SMOKE_PNG")
            .map(PathBuf::from)
            .unwrap_or_else(default_png_path);
        let render_count = Arc::new(AtomicUsize::new(0));
        let outcome = Arc::new(AtomicU8::new(0));
        let app_outcome = outcome.clone();
        let application = Application::try_new().context("initialize native Kael platform")?;

        application.run(move |cx: &mut App| {
            let window = match cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(WIDTH), px(HEIGHT)),
                        cx,
                    ))),
                    titlebar: None,
                    show: false,
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(|_| RendererSmoke {
                        revision: 0,
                        render_count: render_count.clone(),
                    })
                },
            ) {
                Ok(window) => window,
                Err(error) => {
                    eprintln!("NATIVE_RENDERER_SMOKE_FAIL: open real window: {error:#}");
                    app_outcome.store(2, Ordering::Release);
                    cx.quit();
                    return;
                }
            };

            cx.activate(true);
            if let Err(error) = window.update(cx, |_, window, _| {
                // The release proof must remain drawable when launched from an
                // automated terminal whose own window may otherwise occlude it.
                window.set_always_on_top(true);
                window.show_window();
                window.activate_window();
                window.refresh();
            }) {
                eprintln!("NATIVE_RENDERER_SMOKE_FAIL: show real window: {error:#}");
                app_outcome.store(2, Ordering::Release);
                cx.quit();
                return;
            }

            let outcome = app_outcome.clone();
            let render_count = render_count.clone();
            cx.spawn(async move |cx| {
                let initial_deadline = Instant::now() + SMOKE_TIMEOUT;
                while render_count.load(Ordering::Acquire) == 0 {
                    if Instant::now() >= initial_deadline {
                        eprintln!(
                            "NATIVE_RENDERER_SMOKE_FAIL: timed out waiting for the initial retained frame"
                        );
                        outcome.store(2, Ordering::Release);
                        let _ = cx.update(|cx| cx.quit());
                        return;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(16))
                        .await;
                }
                println!(
                    "NATIVE_RENDERER_SMOKE_STAGE: initial render_calls={}",
                    render_count.load(Ordering::Acquire)
                );
                for revision in 1..REQUIRED_RENDER_REVISIONS {
                    let revision_deadline = Instant::now() + SMOKE_TIMEOUT;
                    let prior_count = render_count.load(Ordering::Acquire);
                    if let Err(error) = window.update(cx, |view, window, cx| {
                        view.revision = revision;
                        cx.notify();
                        // A native resize requests an immediate frame on macOS,
                        // so this remains deterministic even when AppKit pauses
                        // the display link for an occluded automation window.
                        // It also proves retained redraws survive viewport changes.
                        window.resize(size(px(WIDTH + revision as f32), px(HEIGHT)));
                    }) {
                        eprintln!(
                            "NATIVE_RENDERER_SMOKE_FAIL: schedule revision {revision}: {error:#}"
                        );
                        outcome.store(2, Ordering::Release);
                        let _ = cx.update(|cx| cx.quit());
                        return;
                    }
                    println!(
                        "NATIVE_RENDERER_SMOKE_STAGE: scheduled revision={revision} prior_render_calls={prior_count} current_render_calls={}",
                        render_count.load(Ordering::Acquire)
                    );
                    while render_count.load(Ordering::Acquire) <= prior_count {
                        if Instant::now() >= revision_deadline {
                            eprintln!(
                                "NATIVE_RENDERER_SMOKE_FAIL: timed out waiting for retained frame revision {revision}"
                            );
                            outcome.store(2, Ordering::Release);
                            let _ = cx.update(|cx| cx.quit());
                            return;
                        }
                        cx.background_executor()
                            .timer(Duration::from_millis(16))
                            .await;
                    }
                }

                // Leave the final scene one compositor interval to present before
                // using the same backend's bounded off-screen readback path.
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let verification = window
                    .update(cx, |_, window, _| verify_frame(window, &output))
                    .context("access native renderer smoke window")
                    .and_then(|result| result);
                match verification {
                    Ok(frame) => {
                        let frames = render_count.load(Ordering::Acquire);
                        if frames < REQUIRED_RENDER_REVISIONS {
                            eprintln!(
                                "NATIVE_RENDERER_SMOKE_FAIL: expected at least {REQUIRED_RENDER_REVISIONS} render calls, observed {frames}"
                            );
                            outcome.store(2, Ordering::Release);
                            let _ = cx.update(|cx| cx.quit());
                            return;
                        }
                        println!(
                            "NATIVE_RENDERER_SMOKE_GPU: backend={} software={} device={:?} driver={:?} info={:?}",
                            backend_name(),
                            frame.software_emulated,
                            frame.device_name,
                            frame.driver_name,
                            frame.driver_info
                        );
                        println!(
                            "NATIVE_RENDERER_SMOKE_FRAMES: revisions={} render_calls={frames}",
                            REQUIRED_RENDER_REVISIONS
                        );
                        println!(
                            "NATIVE_RENDERER_SMOKE_PNG: path={} dimensions={}x{} scale={} bytes={} colors={} text_probe_pixels={} fnv1a64={:016x}",
                            frame.output.display(),
                            frame.width,
                            frame.height,
                            frame.scale_factor,
                            frame.encoded_bytes,
                            frame.distinct_colors,
                            frame.text_probe_pixels,
                            frame.checksum
                        );
                        println!(
                            "NATIVE_RENDERER_SMOKE_OK: real window, retained frames, GPU identity, and device-pixel readback passed"
                        );
                        outcome.store(1, Ordering::Release);
                    }
                    Err(error) => {
                        eprintln!("NATIVE_RENDERER_SMOKE_FAIL: {error:#}");
                        outcome.store(2, Ordering::Release);
                    }
                }
                let _ = cx.update(|cx| cx.quit());
            })
            .detach();
        });

        ensure!(
            outcome.load(Ordering::Acquire) == 1,
            "native renderer smoke failed"
        );
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> anyhow::Result<()> {
    native::run()
}

#[cfg(target_arch = "wasm32")]
fn main() {
    eprintln!("native_renderer_smoke requires a native target");
}

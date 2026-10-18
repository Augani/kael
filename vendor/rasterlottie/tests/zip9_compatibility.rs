//! ZIP 9 dotLottie loader/render compatibility on native Rust and in real browsers.
#![cfg(all(feature = "dotlottie", feature = "images", feature = "text"))]

#[cfg(test)]
mod tests {
    use image::{ColorType, ImageEncoder, codecs::png::PngEncoder};
    use rasterlottie::{
        Animation, DotLottieLimits, ImageLimits, RasterFrame, RenderConfig, Renderer,
    };
    use std::io::{Cursor, Write};
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test;
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    fn archive(entries: &[(&str, &[u8])], method: CompressionMethod) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(method);
        for (name, bytes) in entries {
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn pixel(frame: &RasterFrame, x: usize, y: usize) -> &[u8] {
        let offset = (y * frame.width as usize + x) * 4;
        &frame.pixels[offset..offset + 4]
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn prepared_image_byte_stats_track_lazy_hits_growth_and_failed_admission() {
        use base64::Engine as _;
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&[255, 0, 0, 255], 1, 1, ColorType::Rgba8.into())
            .unwrap();
        let data = base64::engine::general_purpose::STANDARD.encode(png);
        let animation = Animation::from_json_str(&format!(
            r#"{{"fr":30,"ip":0,"op":2,"w":1,"h":1,
            "assets":[{{"id":"first","w":1,"h":1,"e":1,"p":"data:image/png;base64,{data}"}},
                      {{"id":"second","w":1,"h":1,"e":1,"p":"data:image/png;base64,{data}"}}],
            "layers":[{{"ty":2,"ind":1,"refId":"first","ip":0,"op":1}},
                      {{"ty":2,"ind":2,"refId":"second","ip":1,"op":2}}]}}"#
        ))
        .unwrap();
        let prepared = Renderer::target_corpus().prepare(&animation).unwrap();
        assert_eq!(prepared.cached_decoded_image_bytes(), 0);
        for _ in 0..2 {
            assert_eq!(
                prepared
                    .render_frame(0.0, RenderConfig::default())
                    .unwrap()
                    .pixels,
                [255, 0, 0, 255]
            );
            assert_eq!(prepared.cached_decoded_image_bytes(), 4);
        }
        assert_eq!(
            prepared
                .render_frame(1.0, RenderConfig::default())
                .unwrap()
                .pixels,
            [255, 0, 0, 255]
        );
        assert_eq!(prepared.cached_decoded_image_bytes(), 8);
        let mut limits = ImageLimits::default();
        limits.max_total_decoded_bytes = 4;
        let prepared = Renderer::target_corpus()
            .with_image_limits(limits)
            .prepare(&animation)
            .unwrap();
        prepared.render_frame(0.0, RenderConfig::default()).unwrap();
        assert!(prepared.render_frame(1.0, RenderConfig::default()).is_err());
        assert_eq!(prepared.cached_decoded_image_bytes(), 4);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn admission_rejects_manifest_expansion_before_parsing() {
        let mut manifest = br#"{"animations":[{"id":"main"}]}"#.to_vec();
        manifest.resize(1024 * 1024 + 1, b' ');
        let bytes = archive(
            &[
                ("manifest.json", &manifest),
                (
                    "a/main.json",
                    br#"{"fr":30,"ip":0,"op":1,"w":1,"h":1,"layers":[]}"#,
                ),
            ],
            CompressionMethod::Deflated,
        );
        assert!(bytes.len() < 2048);
        assert!(Animation::from_dotlottie_bytes(&bytes).is_err());
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn admission_rejects_oversized_image_target_before_resize() {
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&[255, 0, 0, 255], 1, 1, ColorType::Rgba8.into())
            .unwrap();
        let json = br#"{"fr":30,"ip":0,"op":1,"w":1,"h":1,
            "assets":[{"id":"image","w":8193,"h":1,"p":"red.png"}],
            "layers":[{"ty":2,"ind":1,"refId":"image"}]}"#;
        let bytes = archive(
            &[
                ("manifest.json", br#"{"animations":[{"id":"main"}]}"#),
                ("a/main.json", json),
                ("red.png", &png),
            ],
            CompressionMethod::Deflated,
        );
        let animation = Animation::from_dotlottie_bytes(&bytes).unwrap();
        assert!(
            Renderer::default()
                .render_frame(&animation, 0.0, RenderConfig::default())
                .is_err()
        );
        let mut limits = ImageLimits::default();
        limits.max_dimension = 8193;
        let frame = Renderer::default()
            .with_image_limits(limits)
            .render_frame(&animation, 0.0, RenderConfig::default())
            .unwrap();
        assert_eq!(frame.pixels, [255, 0, 0, 255]);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn admission_public_policy_includes_triangle_sampling_weights() {
        let mut tall_png = Vec::new();
        PngEncoder::new(&mut tall_png)
            .write_image(&[255; 8192], 1, 8192, ColorType::L8.into())
            .unwrap();
        let tall_json = br#"{"fr":30,"ip":0,"op":1,"w":1,"h":1,
            "assets":[{"id":"image","w":1,"h":1,"p":"pixel.png"}],
            "layers":[{"ty":2,"ind":1,"refId":"image"}]}"#;
        let bytes = archive(
            &[
                ("manifest.json", br#"{"animations":[{"id":"main"}]}"#),
                ("a/main.json", tall_json),
                ("pixel.png", &tall_png),
            ],
            CompressionMethod::Deflated,
        );
        let animation = Animation::from_dotlottie_bytes(&bytes).unwrap();
        let mut limits = ImageLimits::default();
        limits.max_working_bytes = 60 * 1024;
        assert!(
            Renderer::default()
                .with_image_limits(limits)
                .render_frame(&animation, 0.0, RenderConfig::default())
                .unwrap_err()
                .to_string()
                .contains("working bytes")
        );
        limits.max_working_bytes = 128 * 1024;
        assert_eq!(
            Renderer::default()
                .with_image_limits(limits)
                .render_frame(&animation, 0.0, RenderConfig::default())
                .unwrap()
                .pixels,
            [255, 255, 255, 255]
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn admission_archive_limits_are_inclusive_and_ignore_unselected_contents() {
        let manifest = br#"{"animations":[{"id":"main"}]}"#;
        let json = br#"{"fr":30,"ip":0,"op":1,"w":1,"h":1,"layers":[]}"#;
        for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
            let bytes = archive(
                &[
                    ("manifest.json", manifest),
                    ("a/main.json", json),
                    ("unused.bin", &[0; 1024]),
                ],
                method,
            );
            let mut limits = DotLottieLimits::default();
            limits.max_archive_bytes = bytes.len() as u64;
            limits.max_manifest_bytes = manifest.len() as u64;
            limits.max_animation_bytes = json.len() as u64;
            limits.max_total_expanded_bytes = (manifest.len() + json.len()) as u64;
            assert_eq!(
                Animation::from_dotlottie_bytes_with_limits(&bytes, limits)
                    .unwrap()
                    .width,
                1
            );
            for field in 0..5 {
                let mut limited = limits;
                match field {
                    0 => limited.max_archive_bytes -= 1,
                    1 => limited.max_manifest_bytes -= 1,
                    2 => limited.max_animation_bytes -= 1,
                    3 => limited.max_entries = 2,
                    _ => limited.max_total_expanded_bytes -= 1,
                }
                assert!(Animation::from_dotlottie_bytes_with_limits(&bytes, limited).is_err());
            }
            limits.max_total_expanded_bytes = 0;
            assert!(Animation::from_dotlottie_bytes_with_limits(&bytes, limits).is_err());
        }
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn admission_charges_each_repeated_image_embedding() {
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&[255, 0, 0, 255], 1, 1, ColorType::Rgba8.into())
            .unwrap();
        let manifest = br#"{"animations":[{"id":"main"}]}"#;
        let json = br#"{"fr":30,"ip":0,"op":1,"w":1,"h":1,"layers":[],"assets":[
            {"id":"first","w":1,"h":1,"p":"pixel.png"},
            {"id":"second","w":1,"h":1,"p":"pixel.png"}]}"#;
        for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
            let bytes = archive(
                &[
                    ("manifest.json", manifest),
                    ("a/main.json", json),
                    ("pixel.png", &png),
                ],
                method,
            );
            let mut limits = DotLottieLimits::default();
            limits.max_image_bytes = png.len() as u64;
            limits.max_total_expanded_bytes = (manifest.len() + json.len() + png.len() * 2) as u64;
            let animation = Animation::from_dotlottie_bytes_with_limits(&bytes, limits).unwrap();
            assert_eq!(animation.assets[0].path, animation.assets[1].path);
            limits.max_total_expanded_bytes -= 1;
            let error = Animation::from_dotlottie_bytes_with_limits(&bytes, limits).unwrap_err();
            assert!(error.to_string().contains("pixel.png"));
            limits.max_total_expanded_bytes += 1;
            limits.max_image_bytes -= 1;
            assert!(Animation::from_dotlottie_bytes_with_limits(&bytes, limits).is_err());
        }
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn admission_preserves_crc_and_forged_size_errors() {
        let manifest = br#"{"animations":[{"id":"main"}]}"#;
        let json = br#"{"fr":30,"ip":0,"op":1,"w":1,"h":1,"layers":[]}"#;
        let bytes = archive(
            &[("manifest.json", manifest), ("a/main.json", json)],
            CompressionMethod::Stored,
        );
        let mut bad_crc = bytes.clone();
        let offset = bad_crc
            .windows(manifest.len())
            .position(|part| part == manifest)
            .unwrap();
        bad_crc[offset + 1] ^= 1;
        assert!(
            Animation::from_dotlottie_bytes(&bad_crc)
                .unwrap_err()
                .to_string()
                .contains("failed to read")
        );
        let mut bad_size = bytes;
        let central = bad_size
            .windows(4)
            .enumerate()
            .filter_map(|(offset, part)| (part == b"PK\x01\x02").then_some(offset))
            .next_back()
            .unwrap();
        let local =
            u32::from_le_bytes(bad_size[central + 42..central + 46].try_into().unwrap()) as usize;
        bad_size[central + 24..central + 28].copy_from_slice(&8u32.to_le_bytes());
        bad_size[local + 22..local + 26].copy_from_slice(&8u32.to_le_bytes());
        assert!(
            Animation::from_dotlottie_bytes(&bad_size)
                .unwrap_err()
                .to_string()
                .contains("failed to read")
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn admission_rejects_duplicate_central_directory_names() {
        let manifest = br#"{"animations":[{"id":"main"}]}"#;
        let json = br#"{"fr":30,"ip":0,"op":1,"w":1,"h":1,"layers":[]}"#;
        let mut bytes = archive(
            &[
                ("manifest.json", manifest),
                ("a/main.json", json),
                ("b/main.json", json),
            ],
            CompressionMethod::Stored,
        );
        let offsets: Vec<_> = bytes
            .windows(11)
            .enumerate()
            .filter_map(|(offset, part)| (part == b"b/main.json").then_some(offset))
            .collect();
        assert_eq!(offsets.len(), 2);
        for offset in offsets {
            bytes[offset] = b'a';
        }
        assert!(Animation::from_dotlottie_bytes(&bytes).is_err());
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn admission_preserves_zip64_entry_headers_and_archive_comments() {
        for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
            let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
            writer
                .set_comment("A normal dotLottie archive comment")
                .unwrap();
            let options = SimpleFileOptions::default()
                .compression_method(method)
                .large_file(true);
            writer.start_file("manifest.json", options).unwrap();
            writer
                .write_all(br#"{"animations":[{"id":"main"}]}"#)
                .unwrap();
            writer.start_file("a/main.json", options).unwrap();
            writer
                .write_all(br#"{"fr":30,"ip":0,"op":1,"w":1,"h":1,"layers":[]}"#)
                .unwrap();
            let bytes = writer.finish().unwrap().into_inner();
            let animation = Animation::from_dotlottie_bytes(&bytes).unwrap();
            assert_eq!(
                Renderer::default()
                    .render_frame(&animation, 0.0, RenderConfig::default())
                    .unwrap()
                    .pixels,
                [0, 0, 0, 0]
            );
        }
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn admission_prepared_policy_guards_late_images_without_charging_cache_hits() {
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&[255, 0, 0, 255], 1, 1, ColorType::Rgba8.into())
            .unwrap();
        let json = br#"{"fr":30,"ip":0,"op":2,"w":1,"h":1,
            "assets":[{"id":"first","w":1,"h":1,"p":"pixel.png"},
                      {"id":"second","w":1,"h":1,"p":"pixel.png"}],
            "layers":[{"ty":2,"ind":1,"refId":"first","ip":0,"op":1},
                      {"ty":2,"ind":2,"refId":"second","ip":1,"op":2}]}"#;
        let bytes = archive(
            &[
                ("manifest.json", br#"{"animations":[{"id":"main"}]}"#),
                ("a/main.json", json),
                ("pixel.png", &png),
            ],
            CompressionMethod::Deflated,
        );
        let animation = Animation::from_dotlottie_bytes(&bytes).unwrap();
        let mut limits = ImageLimits::default();
        limits.max_total_decoded_bytes = 4;
        let prepared = Renderer::default()
            .with_image_limits(limits)
            .prepare(&animation)
            .unwrap();
        for _ in 0..2 {
            assert_eq!(
                prepared
                    .render_frame(0.0, RenderConfig::default())
                    .unwrap()
                    .pixels,
                [255, 0, 0, 255]
            );
        }
        let error = prepared
            .render_frame(1.0, RenderConfig::default())
            .unwrap_err();
        assert!(error.to_string().contains("second"));
        assert!(error.to_string().contains("aggregate decoded"));
        assert_eq!(
            prepared
                .render_frame(0.0, RenderConfig::default())
                .unwrap()
                .pixels,
            [255, 0, 0, 255]
        );
        limits.max_total_decoded_bytes = 8;
        let prepared = Renderer::default()
            .with_image_limits(limits)
            .prepare(&animation)
            .unwrap();
        assert_eq!(
            prepared
                .render_frame(0.0, RenderConfig::default())
                .unwrap()
                .pixels,
            [255, 0, 0, 255]
        );
        assert_eq!(
            prepared
                .render_frame(1.0, RenderConfig::default())
                .unwrap()
                .pixels,
            [255, 0, 0, 255]
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn admission_public_policy_bounds_cross_aspect_work_and_preserves_unused_lazy_assets() {
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(
                &[255, 0, 0, 255].repeat(256),
                256,
                1,
                ColorType::Rgba8.into(),
            )
            .unwrap();
        let json = br#"{"fr":30,"ip":0,"op":1,"w":1,"h":1,
            "assets":[{"id":"image","w":1,"h":256,"p":"pixel.png"},
                      {"id":"unused","w":1,"h":1,"e":1,"p":"not-valid-base64"}],
            "layers":[{"ty":2,"ind":1,"refId":"image"}]}"#;
        let bytes = archive(
            &[
                ("manifest.json", br#"{"animations":[{"id":"main"}]}"#),
                ("a/main.json", json),
                ("pixel.png", &png),
            ],
            CompressionMethod::Deflated,
        );
        let animation = Animation::from_dotlottie_bytes(&bytes).unwrap();
        let mut limits = ImageLimits::default();
        limits.max_working_bytes = 128 * 1024;
        let prepared = Renderer::default()
            .with_image_limits(limits)
            .prepare(&animation)
            .unwrap();
        assert!(
            prepared
                .render_frame(0.0, RenderConfig::default())
                .unwrap_err()
                .to_string()
                .contains("working bytes")
        );
        limits.max_working_bytes = 2 * 1024 * 1024;
        let frame = Renderer::default()
            .with_image_limits(limits)
            .render_frame(&animation, 0.0, RenderConfig::default())
            .unwrap();
        assert_eq!(frame.pixels, [255, 0, 0, 255]);
    }

    #[cfg(target_pointer_width = "32")]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn admission_raised_limits_still_reject_unrepresentable_target_buffers() {
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&[255, 0, 0, 255], 1, 1, ColorType::Rgba8.into())
            .unwrap();
        let json = br#"{"fr":30,"ip":0,"op":1,"w":1,"h":1,
            "assets":[{"id":"image","w":600000000,"h":1,"p":"pixel.png"}],
            "layers":[{"ty":2,"ind":1,"refId":"image"}]}"#;
        let bytes = archive(
            &[
                ("manifest.json", br#"{"animations":[{"id":"main"}]}"#),
                ("a/main.json", json),
                ("pixel.png", &png),
            ],
            CompressionMethod::Deflated,
        );
        let animation = Animation::from_dotlottie_bytes(&bytes).unwrap();
        let mut limits = ImageLimits::default();
        limits.max_dimension = u32::MAX;
        limits.max_rgba_bytes = u64::MAX;
        limits.max_working_bytes = u64::MAX;
        limits.max_total_decoded_bytes = u64::MAX;
        let error = Renderer::default()
            .with_image_limits(limits)
            .render_frame(&animation, 0.0, RenderConfig::default())
            .unwrap_err();
        assert!(error.to_string().contains("RGBA bytes"));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn stored_and_deflated_manifest_select_the_initial_animation() {
        for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
            let bytes = archive(&[
                ("manifest.json", br#"{"version":"2","animations":[{"id":"first"},{"id":"second"}],"initial":{"animation":"second"}}"#),
                ("a/first.json", br#"{"fr":30,"ip":0,"op":10,"w":16,"h":16,"layers":[]}"#),
                ("a/second.json", br#"{"fr":30,"ip":0,"op":10,"w":24,"h":32,"layers":[]}"#),
            ], method);
            let animation = Animation::from_dotlottie_bytes(&bytes).unwrap();
            let frame = Renderer::default()
                .render_frame(&animation, 0.0, RenderConfig::default())
                .unwrap();
            assert_eq!(
                (frame.width, frame.height, frame.pixels.len()),
                (24, 32, 24 * 32 * 4)
            );
        }
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn stored_and_deflated_dotlottie_render_archive_image_pixels() {
        let pixels = [255, 0, 0, 255].repeat(16 * 16);
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&pixels, 16, 16, ColorType::Rgba8.into())
            .unwrap();
        let json = br#"{
            "v":"5.7.6","fr":30,"ip":0,"op":10,"w":16,"h":16,
            "assets":[{"id":"img","w":16,"h":16,"u":"images/","p":"red.png"}],
            "layers":[{"nm":"Image","ind":1,"ty":2,"refId":"img","ks":{
                "a":{"a":0,"k":[0,0]},"p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},
                "r":{"a":0,"k":0},"o":{"a":0,"k":100}}}]}"#;
        for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
            let bytes = archive(
                &[
                    ("manifest.json", br#"{"animations":[{"id":"image"}]}"#),
                    ("animations/image.json", json),
                    ("images/red.png", &png),
                ],
                method,
            );
            let animation = Animation::from_dotlottie_bytes(&bytes).unwrap();
            let frame = Renderer::default()
                .render_frame(&animation, 0.0, RenderConfig::default())
                .unwrap();
            assert_eq!(frame.pixels, pixels);
        }
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn stored_and_deflated_dotlottie_render_embedded_glyph_text_pixels() {
        let json = include_bytes!("fixtures/zip9_glyph_text.json");
        for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
            let bytes = archive(
                &[
                    ("manifest.json", br#"{"animations":[{"id":"text"}]}"#),
                    ("a/text.json", json),
                ],
                method,
            );
            let animation = Animation::from_dotlottie_bytes(&bytes).unwrap();
            let frame = Renderer::default()
                .render_frame(&animation, 0.0, RenderConfig::default())
                .unwrap();
            assert_eq!(pixel(&frame, 30, 40), [255, 0, 0, 255]);
            assert_eq!(pixel(&frame, 90, 40), [0, 0, 0, 0]);
        }
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn malformed_and_truncated_archives_return_errors() {
        assert!(Animation::from_dotlottie_bytes(b"not a ZIP archive").is_err());
        let bytes = archive(
            &[
                ("manifest.json", br#"{"animations":[{"id":"text"}]}"#),
                (
                    "a/text.json",
                    include_bytes!("fixtures/zip9_glyph_text.json"),
                ),
            ],
            CompressionMethod::Deflated,
        );
        assert!(Animation::from_dotlottie_bytes(&bytes[..bytes.len() / 2]).is_err());
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn prepared_choker_numeric_limits_render_tiny_frames() {
        for amount in [
            0.0,
            1.0,
            -1.0,
            2_147_483_648.0,
            -2_147_483_648.0,
            2_147_483_904.0,
            -2_147_483_904.0,
        ] {
            let value = serde_json::json!({
                "v":"5.7.6", "fr":30, "ip":0, "op":2, "w":2, "h":2,
                "layers":[{"ind":1, "ty":4, "shapes":[], "ef":[{
                    "mn":"ADBE Simple Choker", "en":1, "ef":[
                        {"mn":"ADBE Simple Choker-0001", "v":{"a":0,"k":1}},
                        {"mn":"ADBE Simple Choker-0002", "v":{"a":0,"k":amount}}
                    ]
                }]}]
            });
            let animation = Animation::from_json_str(&value.to_string()).unwrap();
            let renderer = Renderer::target_corpus();
            assert!(renderer.analyze(&animation).is_supported());
            let frame = renderer
                .prepare(&animation)
                .unwrap()
                .render_frame(0.0, RenderConfig::default())
                .unwrap();
            assert_eq!((frame.width, frame.height), (2, 2));
            assert_eq!(frame.pixels, [0; 16], "amount={amount}");
        }
    }
}

//! ZIP 9 dotLottie loader/render compatibility on native Rust and in real browsers.
#![cfg(all(feature = "dotlottie", feature = "images", feature = "text"))]

#[cfg(test)]
mod tests {
    use image::{ColorType, ImageEncoder, codecs::png::PngEncoder};
    use rasterlottie::{Animation, RasterFrame, RenderConfig, Renderer};
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
}

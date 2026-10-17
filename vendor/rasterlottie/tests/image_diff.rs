//! Golden image regression tests for reference fixtures.
#![allow(clippy::panic, reason = "this is test")]

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use image::ImageReader;
    use rasterlottie::{Animation, RasterFrame, RenderConfig, Renderer};

    const REFERENCE_FIXTURES: &[&str] = &[
        "layer_parenting_basic",
        "polystar_basic",
        "stroke_dash_basic",
        "trim_path_basic",
        "repeater_basic",
    ];

    #[test]
    fn reference_goldens_match_with_small_pixel_error() {
        let mut failures = Vec::new();
        for name in REFERENCE_FIXTURES {
            let animation = load_fixture_animation(&format!("{name}.json"));
            let frame = Renderer::default()
                .render_frame(&animation, 0.0, RenderConfig::default())
                .unwrap_or_else(|error| panic!("failed to render fixture {name}: {error}"));
            let reference = load_reference_rgba(name);
            if let Some(reason) = diff_failure(name, &frame, &reference) {
                failures.push(reason);
            }
        }
        assert!(
            failures.is_empty(),
            "image-diff mismatches:\n{}",
            failures.join("\n")
        );
    }

    fn diff_failure(name: &str, actual: &RasterFrame, expected: &RasterFrame) -> Option<String> {
        if (actual.width, actual.height) != (expected.width, expected.height) {
            return Some(format!(
                "{name}: reference dimensions differ: actual=({},{}) expected=({},{})",
                actual.width, actual.height, expected.width, expected.height
            ));
        }

        if actual.pixels == expected.pixels {
            return None;
        }

        let mut changed_pixels = 0u32;
        let mut total_channel_error = 0u64;
        let mut max_channel_error = 0u8;
        for (actual_rgba, expected_rgba) in actual
            .pixels
            .chunks_exact(4)
            .zip(expected.pixels.chunks_exact(4))
        {
            let mut pixel_changed = false;
            for (actual_channel, expected_channel) in actual_rgba.iter().zip(expected_rgba.iter()) {
                let delta = actual_channel.abs_diff(*expected_channel);
                total_channel_error += u64::from(delta);
                max_channel_error = max_channel_error.max(delta);
                if delta > 8 {
                    pixel_changed = true;
                }
            }
            if pixel_changed {
                changed_pixels += 1;
            }
        }

        let pixel_count = u64::from(actual.width) * u64::from(actual.height);
        let mean_channel_error = total_channel_error as f64 / (pixel_count * 4) as f64;
        let changed_ratio = changed_pixels as f64 / pixel_count as f64;
        Some(format!(
            "{name}: mean_channel_error={mean_channel_error:.3}, changed_ratio={changed_ratio:.3}, max_channel_error={max_channel_error}"
        ))
    }

    fn load_fixture_animation(name: &str) -> Animation {
        let path = fixture_path(name);
        let json = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read fixture {}: {error}", path.display()));
        Animation::from_json_str(&json)
            .unwrap_or_else(|error| panic!("failed to parse fixture {}: {error}", path.display()))
    }

    fn load_reference_rgba(name: &str) -> RasterFrame {
        let path = reference_path(name);
        let image = ImageReader::open(&path)
            .unwrap_or_else(|error| panic!("failed to open reference {}: {error}", path.display()))
            .decode()
            .unwrap_or_else(|error| {
                panic!("failed to decode reference {}: {error}", path.display())
            })
            .to_rgba8();
        RasterFrame::new(image.width(), image.height(), image.into_raw())
    }

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name)
    }

    fn reference_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("reference")
            .join(format!("{name}.png"))
    }
}

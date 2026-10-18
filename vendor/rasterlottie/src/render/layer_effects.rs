use tiny_skia::Pixmap;

use crate::{
    Layer,
    effects::{
        FillEffect, SimpleChokerEffect, SupportedLayerEffect, fill_effect_color,
        fill_effect_opacity, simple_choker_amount, supported_layer_effects,
    },
};

pub(super) fn apply_supported_layer_effects(pixmap: &mut Pixmap, layer: &Layer, frame: f32) {
    span_enter!(
        tracing::Level::TRACE,
        "apply_supported_layer_effects",
        frame = frame,
        layer = layer.name.as_str()
    );
    let Some(effects) = supported_layer_effects(layer) else {
        return;
    };

    for effect in effects {
        match effect {
            SupportedLayerEffect::Fill(fill) => apply_fill_effect(pixmap, &fill, frame),
            SupportedLayerEffect::SimpleChoker(choker) => {
                apply_simple_choker_effect(pixmap, &choker, frame);
            }
        }
    }
}

fn apply_fill_effect(pixmap: &mut Pixmap, effect: &FillEffect, frame: f32) {
    let Some(color) = fill_effect_color(effect, frame) else {
        return;
    };
    let Some(opacity) = fill_effect_opacity(effect, frame) else {
        return;
    };
    let factor = opacity * (f32::from(color[3]) / 255.0);
    if factor <= f32::EPSILON {
        return;
    }

    for pixel in pixmap.data_mut().chunks_exact_mut(4) {
        if pixel[3] == 0 {
            continue;
        }

        pixel[0] = blend_fill_channel(pixel[0], color[0], factor);
        pixel[1] = blend_fill_channel(pixel[1], color[1], factor);
        pixel[2] = blend_fill_channel(pixel[2], color[2], factor);
    }
}

fn blend_fill_channel(original: u8, target: u8, factor: f32) -> u8 {
    let original = f32::from(original);
    let target = f32::from(target);
    (target - original)
        .mul_add(factor.clamp(0.0, 1.0), original)
        .round() as u8
}

fn apply_simple_choker_effect(pixmap: &mut Pixmap, effect: &SimpleChokerEffect, frame: f32) {
    let Some(amount) = simple_choker_amount(effect, frame) else {
        return;
    };
    let rounded = amount.round();
    if !rounded.is_finite() || rounded.abs() > i32::MAX as f32 || rounded == 0.0 {
        return;
    }

    let Ok(width) = i32::try_from(pixmap.width()) else {
        return;
    };
    let Ok(height) = i32::try_from(pixmap.height()) else {
        return;
    };
    let width = width as usize;
    let height = height as usize;
    let erode = rounded > 0.0;
    // Pixmaps have positive dimensions. Larger radii select exactly the same
    // clipped square, including the f32 threshold where i32::MAX rounds to 2^31.
    let radius = (rounded.abs() as usize).min(width.max(height) - 1);
    if radius == width.max(height) - 1 {
        // Every clipped window covers the whole image; RGB remains unchanged.
        let alphas = pixmap.data().chunks_exact(4).map(|pixel| pixel[3]);
        let alpha = if erode { alphas.min() } else { alphas.max() }.unwrap_or(0);
        for pixel in pixmap.data_mut().chunks_exact_mut(4) {
            pixel[3] = alpha;
        }
        return;
    }
    let source = pixmap.data().to_vec();

    for y in 0..height {
        for x in 0..width {
            let mut accumulated = if erode { u8::MAX } else { 0 };
            for sample_y in y.saturating_sub(radius)..=y.saturating_add(radius).min(height - 1) {
                for sample_x in x.saturating_sub(radius)..=x.saturating_add(radius).min(width - 1) {
                    let index = (sample_y * width + sample_x) * 4 + 3;
                    let alpha = source[index];
                    if erode {
                        accumulated = accumulated.min(alpha);
                    } else {
                        accumulated = accumulated.max(alpha);
                    }
                }
            }

            let index = (y * width + x) * 4 + 3;
            pixmap.data_mut()[index] = accumulated;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Animation, RenderConfig, Renderer};
    use serde_json::json;

    fn animation(amount: f64, width: u32, height: u32) -> Animation {
        Animation::from_json_str(
            &json!({
                "v":"5.7.6", "fr":30, "ip":0, "op":2, "w":width, "h":height,
                "layers":[{"nm":"Tiny Choker", "ind":1, "ty":4, "shapes":[], "ef":[{
                    "mn":"ADBE Simple Choker", "en":1, "ef":[
                        {"mn":"ADBE Simple Choker-0001", "v":{"a":0,"k":1}},
                        {"mn":"ADBE Simple Choker-0002", "v":{"a":0,"k":amount}}
                    ]
                }]}]
            })
            .to_string(),
        )
        .unwrap()
    }

    // Enumerate the whole tiny image by geometric distance rather than using
    // the implementation's clipped window endpoints or signed integer radius.
    fn reference(source: &[u8], width: u32, height: u32, amount: f64) -> Vec<u8> {
        let amount = (amount as f32).round();
        let mut result = source.to_vec();
        if !amount.is_finite() || amount.abs() > 2_147_483_648.0 || amount == 0.0 {
            return result;
        }
        for y in 0..height {
            for x in 0..width {
                let mut alpha = if amount > 0.0 { 255 } else { 0 };
                for sy in 0..height {
                    for sx in 0..width {
                        if x.abs_diff(sx).max(y.abs_diff(sy)) as f32 <= amount.abs() {
                            let value = source[((sy * width + sx) * 4 + 3) as usize];
                            alpha = if amount > 0.0 {
                                alpha.min(value)
                            } else {
                                alpha.max(value)
                            };
                        }
                    }
                }
                result[((y * width + x) * 4 + 3) as usize] = alpha;
            }
        }
        result
    }

    #[test]
    fn choker_finite_boundaries_match_geometric_alpha_oracle() {
        for (width, height) in [(1, 1), (2, 3), (3, 2), (1, 4), (4, 1), (2, 2)] {
            let mut original = Pixmap::new(width, height).unwrap();
            for (index, pixel) in original.data_mut().chunks_exact_mut(4).enumerate() {
                pixel.copy_from_slice(&[index as u8, 2, 3, (index * 47) as u8]);
            }
            for amount in [
                0.0,
                -0.0,
                0.49,
                -0.49,
                0.5,
                -0.5,
                1.0,
                -1.0,
                2.0,
                -2.0,
                3.0,
                -3.0,
                2_147_483_520.0,
                -2_147_483_520.0,
                2_147_483_648.0,
                -2_147_483_648.0,
                2_147_483_904.0,
                -2_147_483_904.0,
                1e100,
                -1e100,
            ] {
                let mut pixmap = original.clone();
                let animation = animation(amount, width, height);
                apply_supported_layer_effects(&mut pixmap, &animation.layers[0], 0.0);
                assert_eq!(
                    pixmap.data(),
                    reference(original.data(), width, height, amount),
                    "canvas={width}x{height}, amount={amount}"
                );
            }
        }
    }

    #[test]
    fn prepared_choker_boundaries_preserve_small_asymmetric_pixels() {
        for (width, height) in [(2, 3), (3, 2)] {
            let mut animation = animation(0.0, width, height);
            animation.layers[0].shapes = serde_json::from_value(json!([{"ty":"gr", "it":[
                {"ty":"rc", "p":{"a":0,"k":[0.5,0.5]}, "s":{"a":0,"k":[1,1]}, "r":{"a":0,"k":0}},
                {"ty":"rc", "p":{"a":0,"k":[width as f32-0.5,height as f32-0.5]}, "s":{"a":0,"k":[1,1]}, "r":{"a":0,"k":0}},
                {"ty":"fl", "c":{"a":0,"k":[1,1,1,1]}, "o":{"a":0,"k":100}}
            ]}])).unwrap();
            let renderer = Renderer::target_corpus();
            let source = renderer
                .prepare(&animation)
                .unwrap()
                .render_frame(0.0, RenderConfig::default())
                .unwrap();
            let mut known = vec![0; (width * height * 4) as usize];
            known[..4].fill(255);
            known[(width * height * 4 - 4) as usize..].fill(255);
            assert_eq!(source.pixels, known, "source fixture pixels");
            for amount in [
                0.0,
                1.0,
                -1.0,
                2_147_483_648.0,
                -2_147_483_648.0,
                2_147_483_904.0,
                -2_147_483_904.0,
                1e100,
                -1e100,
            ] {
                animation.layers[0].effects[0]["ef"][1]["v"]["k"] = json!(amount);
                assert!(renderer.analyze(&animation).is_supported());
                let frame = renderer
                    .prepare(&animation)
                    .unwrap()
                    .render_frame(0.0, RenderConfig::default())
                    .unwrap();
                let mut expected = reference(&known, width, height, amount);
                // Existing alpha-only effect behavior is tested, not broader
                // premultiplication semantics. These binary-alpha patterns
                // fully disappear under erosion; added dilation pixels are black.
                for pixel in expected.chunks_exact_mut(4) {
                    if pixel[3] == 0 {
                        pixel.fill(0);
                    }
                }
                assert_eq!(frame.width, width);
                assert_eq!(frame.height, height);
                assert_eq!(
                    frame.pixels, expected,
                    "canvas={width}x{height}, amount={amount}"
                );
            }
        }
    }
}

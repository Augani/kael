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
    if !rounded.is_finite() || rounded.abs() > i32::MAX as f32 {
        return;
    }
    let radius = rounded as i32;
    if radius == 0 {
        return;
    }

    let Ok(width) = i32::try_from(pixmap.width()) else {
        return;
    };
    let Ok(height) = i32::try_from(pixmap.height()) else {
        return;
    };
    let source = pixmap.data().to_vec();

    for y in 0..height {
        for x in 0..width {
            let mut accumulated = if radius > 0 { u8::MAX } else { 0 };
            for sample_y in (y - radius.abs()).max(0)..=(y + radius.abs()).min(height - 1) {
                for sample_x in (x - radius.abs()).max(0)..=(x + radius.abs()).min(width - 1) {
                    let index = ((sample_y * width + sample_x) * 4 + 3) as usize;
                    let alpha = source[index];
                    if radius > 0 {
                        accumulated = accumulated.min(alpha);
                    } else {
                        accumulated = accumulated.max(alpha);
                    }
                }
            }

            let index = ((y * width + x) * 4 + 3) as usize;
            pixmap.data_mut()[index] = accumulated;
        }
    }
}

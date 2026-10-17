use serde_json::Value;

#[cfg(feature = "dotlottie")]
use super::dotlottie::load_animation_from_dotlottie_bytes;
use super::{
    RasterlottieError,
    model_parse::{color_components_to_rgba, f32_unit_to_u8, parse_bezier_path},
};
pub use super::{model_parse::parse_bezier_keyframe_value, model_types::*};

impl Animation {
    /// Parses a Lottie animation from a JSON string.
    ///
    /// # Errors
    ///
    /// Returns an error when `json` is not valid Lottie JSON for this model.
    pub fn from_json_str(json: &str) -> Result<Self, RasterlottieError> {
        Ok(serde_json::from_str(json)?)
    }

    /// Parses the primary animation from a `.lottie` archive.
    ///
    /// The loader prefers the manifest's explicitly selected animation and
    /// otherwise falls back to the first listed animation entry.
    ///
    /// # Errors
    ///
    /// Returns an error when `dotlottie` is not a valid `.lottie` archive or
    /// when the selected embedded Lottie JSON cannot be parsed.
    #[cfg(feature = "dotlottie")]
    #[cfg_attr(docsrs, doc(cfg(feature = "dotlottie")))]
    pub fn from_dotlottie_bytes(dotlottie: &[u8]) -> Result<Self, RasterlottieError> {
        load_animation_from_dotlottie_bytes(dotlottie)
    }

    /// Returns the duration in frames.
    #[must_use]
    pub fn duration_frames(&self) -> f32 {
        (self.out_point - self.in_point).max(0.0)
    }

    /// Returns the duration in seconds.
    #[must_use]
    pub fn duration_seconds(&self) -> f32 {
        if self.frame_rate <= f32::EPSILON {
            0.0
        } else {
            self.duration_frames() / self.frame_rate
        }
    }

    /// Finds a font by its Lottie font name.
    #[must_use]
    pub fn lookup_font(&self, name: &str) -> Option<&Font> {
        self.fonts.list.iter().find(|font| font.name == name)
    }

    /// Finds a glyph entry that matches the given grapheme and font.
    #[must_use]
    pub fn lookup_glyph(&self, grapheme: &str, font: &Font) -> Option<&FontCharacter> {
        self.chars
            .iter()
            .find(|glyph| glyph.matches(grapheme, font))
    }
}

impl FontCharacter {
    /// Returns `true` when this glyph matches the provided grapheme and font metadata.
    #[must_use]
    pub fn matches(&self, grapheme: &str, font: &Font) -> bool {
        self.character == grapheme
            && (self.style.is_empty() || font.style.is_empty() || self.style == font.style)
            && self
                .family
                .as_deref()
                .is_none_or(|family| family.is_empty() || family == font.family)
    }

    /// Returns the scaled advance width for the requested font size.
    #[must_use]
    pub fn advance_for_size(&self, size: f32) -> Option<f32> {
        (self.size > f32::EPSILON).then_some((self.width * size) / self.size)
    }
}

impl Asset {
    /// Returns `true` when this asset references image data.
    #[must_use]
    pub const fn is_image_asset(&self) -> bool {
        self.path.is_some()
    }

    /// Returns `true` when this asset stores image data inline as a data URL.
    #[must_use]
    pub fn is_embedded_image_asset(&self) -> bool {
        self.is_image_asset() && self.embedded.unwrap_or(0) != 0
    }

    /// Returns the embedded data URL for inline image assets.
    #[must_use]
    pub fn image_data_url(&self) -> Option<&str> {
        self.is_embedded_image_asset()
            .then_some(self.path.as_deref())
            .flatten()
    }
}

impl LayerType {
    /// Image layer.
    pub const IMAGE: Self = Self(2);
    /// Null layer.
    pub const NULL: Self = Self(3);
    /// Precomposition layer.
    pub const PRECOMP: Self = Self(0);
    /// Shape layer.
    pub const SHAPE: Self = Self(4);
    /// Solid-color layer.
    pub const SOLID: Self = Self(1);
    /// Text layer.
    pub const TEXT: Self = Self(5);

    /// Returns a human-readable name for the layer type.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self.0 {
            0 => "precomp",
            1 => "solid",
            2 => "image",
            3 => "null",
            4 => "shape",
            5 => "text",
            _ => "unknown",
        }
    }
}

impl Layer {
    /// Decodes the layer's track matte mode tag.
    #[must_use]
    pub fn track_matte_mode(&self) -> Option<TrackMatteMode> {
        match self.track_matte? {
            1 => Some(TrackMatteMode::Alpha),
            2 => Some(TrackMatteMode::AlphaInverted),
            3 => Some(TrackMatteMode::Luma),
            4 => Some(TrackMatteMode::LumaInverted),
            _ => None,
        }
    }

    /// Returns `true` when this layer acts as a matte source.
    #[must_use]
    pub fn is_matte_source_layer(&self) -> bool {
        self.matte_source == Some(1)
    }
}

impl TextData {
    /// Returns the text document active at `frame`.
    #[must_use]
    pub fn document_at(&self, frame: f32) -> Option<&TextDocument> {
        let mut current = self.document.keyframes.first()?;
        for keyframe in &self.document.keyframes[1..] {
            if keyframe.time > frame {
                break;
            }
            current = keyframe;
        }
        Some(&current.document)
    }

    /// Returns `true` when the text layer defines animators.
    #[must_use]
    pub const fn has_animators(&self) -> bool {
        !self.animators.is_empty()
    }

    /// Returns `true` when the text layer defines a text path.
    #[must_use]
    pub fn has_path(&self) -> bool {
        match &self.path {
            Value::Null => false,
            Value::Object(object) => !object.is_empty(),
            Value::Array(items) => !items.is_empty(),
            _ => true,
        }
    }
}

impl TextDocument {
    /// Converts the fill color into RGBA8 if enough components are present.
    #[must_use]
    pub fn fill_color_rgba(&self) -> Option<[u8; 4]> {
        color_components_to_rgba(&self.fill_color)
    }

    /// Converts the stroke color into RGBA8 if enough components are present.
    #[must_use]
    pub fn stroke_color_rgba(&self) -> Option<[u8; 4]> {
        color_components_to_rgba(&self.stroke_color)
    }

    /// Returns the effective line height, falling back to a size-based default.
    #[must_use]
    pub fn effective_line_height(&self) -> f32 {
        if self.line_height > f32::EPSILON {
            self.line_height
        } else {
            self.size * 1.2
        }
    }
}

impl Mask {
    /// Decodes the raw mask mode string.
    #[must_use]
    pub fn mask_mode(&self) -> Option<MaskMode> {
        let mode = self.mode.as_deref()?.trim().to_ascii_lowercase();
        match mode.as_str() {
            "a" | "add" => Some(MaskMode::Add),
            "s" | "subtract" => Some(MaskMode::Subtract),
            "i" | "intersect" => Some(MaskMode::Intersect),
            "n" | "none" => Some(MaskMode::None),
            _ => None,
        }
    }
}

impl AnimatedValue {
    /// Returns `true` when the property carries an expression.
    #[must_use]
    pub const fn has_expression(&self) -> bool {
        self.expression.is_some()
    }

    /// Returns `true` when the property is stored as a static literal value.
    #[must_use]
    pub fn is_static(&self) -> bool {
        let Some(keyframes) = self.keyframes.as_ref() else {
            return false;
        };

        if self.animated == Some(1) {
            return false;
        }

        match keyframes {
            Value::Number(_) => true,
            Value::Array(items) => items.iter().all(Value::is_number),
            _ => false,
        }
    }

    /// Returns the static value as a scalar.
    #[must_use]
    pub fn as_scalar(&self) -> Option<f32> {
        let values = self.as_vec()?;
        (values.len() == 1).then_some(values[0])
    }

    /// Returns the static value as a 2D vector.
    #[must_use]
    pub fn as_vec2(&self) -> Option<[f32; 2]> {
        let values = self.as_vec()?;
        (values.len() >= 2).then_some([values[0], values[1]])
    }

    /// Returns the static value as a 3D vector.
    #[must_use]
    pub fn as_vec3(&self) -> Option<[f32; 3]> {
        let values = self.as_vec()?;
        (values.len() >= 3).then_some([values[0], values[1], values[2]])
    }

    /// Returns the static value as RGBA8 color components.
    #[must_use]
    pub fn as_color_rgba(&self) -> Option<[u8; 4]> {
        let values = self.as_vec()?;
        if values.len() < 3 {
            return None;
        }

        let alpha = values.get(3).copied().unwrap_or(1.0);
        Some([
            f32_unit_to_u8(values[0]),
            f32_unit_to_u8(values[1]),
            f32_unit_to_u8(values[2]),
            f32_unit_to_u8(alpha),
        ])
    }

    fn as_vec(&self) -> Option<Vec<f32>> {
        if !self.is_static() {
            return None;
        }

        match self.keyframes.as_ref()? {
            Value::Number(number) => Some(vec![number.as_f64()? as f32]),
            Value::Array(items) => items
                .iter()
                .map(|item| item.as_f64().map(|value| value as f32))
                .collect(),
            _ => None,
        }
    }
}

impl PositionValue {
    /// Returns the combined-value form when available.
    #[must_use]
    pub const fn combined(&self) -> Option<&AnimatedValue> {
        match self {
            Self::Combined(value) => Some(value),
            Self::Split(_) => None,
        }
    }

    /// Returns the split-axis form when available.
    #[must_use]
    pub const fn split(&self) -> Option<&SplitPosition> {
        match self {
            Self::Combined(_) => None,
            Self::Split(value) => Some(value),
        }
    }
}

impl SplitPosition {
    /// Returns `true` when the property is explicitly marked as split.
    #[must_use]
    pub fn is_split(&self) -> bool {
        self.split.unwrap_or(0) != 0
    }
}

impl ShapePathValue {
    /// Returns `true` when the property carries an expression.
    #[must_use]
    pub const fn has_expression(&self) -> bool {
        self.expression.is_some()
    }

    /// Returns `true` when the property is stored as one static path object.
    #[must_use]
    pub fn is_static(&self) -> bool {
        let Some(keyframes) = self.keyframes.as_ref() else {
            return false;
        };

        if self.animated == Some(1) {
            return false;
        }

        keyframes.is_object()
    }

    /// Returns the static path as a decoded cubic Bezier path.
    #[must_use]
    pub fn as_bezier_path(&self) -> Option<BezierPath> {
        if !self.is_static() {
            return None;
        }

        parse_bezier_path(self.keyframes.as_ref()?)
    }
}

impl ShapeItem {
    /// Returns `true` when the item carries an expression.
    #[must_use]
    pub const fn has_expression(&self) -> bool {
        self.expression.is_some()
    }

    /// Decodes the gradient payload stored under `g`.
    #[must_use]
    pub fn gradient_data(&self) -> Option<GradientData> {
        self.extra
            .get("g")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
    }

    /// Returns the raw gradient type tag.
    pub fn gradient_type(&self) -> Option<u8> {
        self.extra
            .get("t")
            .and_then(Value::as_u64)
            .and_then(|value| u8::try_from(value).ok())
    }

    /// Returns the gradient start point property.
    #[must_use]
    pub const fn gradient_start_point(&self) -> Option<&AnimatedValue> {
        self.size.as_ref()
    }

    /// Returns the gradient end point property.
    #[must_use]
    pub fn gradient_end_point(&self) -> Option<AnimatedValue> {
        self.extra
            .get("e")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
    }

    /// Returns the radial-gradient highlight length property.
    #[must_use]
    pub fn gradient_highlight_length(&self) -> Option<AnimatedValue> {
        self.extra
            .get("h")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
    }

    /// Returns the radial-gradient highlight angle property.
    #[must_use]
    pub const fn gradient_highlight_angle(&self) -> Option<&AnimatedValue> {
        self.anchor.as_ref()
    }

    /// Returns the raw `r` property used by several item kinds.
    #[must_use]
    pub fn raw_r_value(&self) -> Option<AnimatedValue> {
        self.extra
            .get("r")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
    }

    /// Returns the rectangle roundness property.
    #[must_use]
    pub fn rectangle_roundness(&self) -> Option<AnimatedValue> {
        self.raw_r_value()
    }

    /// Returns the transform rotation property.
    #[must_use]
    pub fn transform_rotation(&self) -> Option<AnimatedValue> {
        self.raw_r_value()
    }

    /// Decodes the stroke dash pattern definition.
    #[must_use]
    pub fn dash_pattern(&self) -> Option<Vec<DashPatternEntry>> {
        self.extra
            .get("d")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
    }

    /// Returns the trim-path end property.
    #[must_use]
    pub fn trim_end(&self) -> Option<AnimatedValue> {
        self.extra
            .get("e")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
    }

    /// Returns the merge-path mode tag.
    pub fn merge_mode(&self) -> Option<u8> {
        self.extra
            .get("mm")
            .and_then(Value::as_u64)
            .and_then(|value| u8::try_from(value).ok())
    }

    /// Returns the trim-path mode tag.
    pub fn trim_mode(&self) -> Option<u8> {
        self.extra
            .get("m")
            .and_then(Value::as_u64)
            .and_then(|value| u8::try_from(value).ok())
    }

    /// Returns the trim-path start property.
    #[must_use]
    pub const fn trim_start(&self) -> Option<&AnimatedValue> {
        self.size.as_ref()
    }

    /// Returns the trim-path offset property.
    #[must_use]
    pub const fn trim_offset(&self) -> Option<&AnimatedValue> {
        self.opacity.as_ref()
    }

    /// Returns the polystar subtype tag.
    pub fn polystar_type(&self) -> Option<u8> {
        self.extra
            .get("sy")
            .and_then(Value::as_u64)
            .and_then(|value| u8::try_from(value).ok())
    }

    /// Returns the polystar point-count property.
    #[must_use]
    pub fn polystar_points(&self) -> Option<AnimatedValue> {
        self.extra
            .get("pt")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
    }

    /// Returns the polystar outer-radius property.
    #[must_use]
    pub fn polystar_outer_radius(&self) -> Option<AnimatedValue> {
        self.extra
            .get("or")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
    }

    /// Returns the polystar outer-roundness property.
    #[must_use]
    pub fn polystar_outer_roundness(&self) -> Option<AnimatedValue> {
        self.extra
            .get("os")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
    }

    /// Returns the polystar inner-radius property.
    #[must_use]
    pub fn polystar_inner_radius(&self) -> Option<AnimatedValue> {
        self.extra
            .get("ir")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
    }

    /// Returns the polystar inner-roundness property.
    #[must_use]
    pub fn polystar_inner_roundness(&self) -> Option<AnimatedValue> {
        self.extra
            .get("is")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
    }

    /// Returns the shape direction tag.
    pub fn shape_direction(&self) -> Option<u8> {
        self.extra
            .get("d")
            .and_then(Value::as_u64)
            .and_then(|value| u8::try_from(value).ok())
    }

    /// Returns the repeater copy-count property.
    #[must_use]
    pub const fn repeater_copies(&self) -> Option<&AnimatedValue> {
        self.color.as_ref()
    }

    /// Returns the polystar rotation property.
    #[must_use]
    pub fn polystar_rotation(&self) -> Option<AnimatedValue> {
        self.raw_r_value()
    }

    /// Returns the repeater offset property.
    #[must_use]
    pub const fn repeater_offset(&self) -> Option<&AnimatedValue> {
        self.opacity.as_ref()
    }

    /// Returns the repeater composite-mode tag.
    pub fn repeater_composite_mode(&self) -> Option<u8> {
        self.extra
            .get("m")
            .and_then(Value::as_u64)
            .and_then(|value| u8::try_from(value).ok())
    }

    /// Decodes the repeater transform block.
    #[must_use]
    pub fn repeater_transform(&self) -> Option<RepeaterTransform> {
        self.extra
            .get("tr")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
    }
}

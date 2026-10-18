use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

/// Root Lottie animation document.
#[derive(Debug, Clone, Deserialize)]
pub struct Animation {
    #[serde(default, rename = "v")]
    /// Lottie format version string.
    pub version: String,
    #[serde(rename = "fr")]
    /// Animation frame rate in frames per second.
    pub frame_rate: f32,
    #[serde(rename = "ip")]
    /// Inclusive start frame.
    pub in_point: f32,
    #[serde(rename = "op")]
    /// Exclusive end frame.
    pub out_point: f32,
    #[serde(rename = "w")]
    /// Canvas width in pixels.
    pub width: u32,
    #[serde(rename = "h")]
    /// Canvas height in pixels.
    pub height: u32,
    #[serde(default)]
    /// Top-level layers in stacking order.
    pub layers: Vec<Layer>,
    #[serde(default)]
    /// External assets and precompositions referenced by the animation.
    pub assets: Vec<Asset>,
    #[serde(default)]
    /// Embedded font metadata.
    pub fonts: FontList,
    #[serde(default)]
    /// Embedded glyph outlines.
    pub chars: Vec<FontCharacter>,
}

/// Font metadata table embedded in a Lottie document.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct FontList {
    #[serde(default, rename = "list")]
    /// Registered fonts.
    pub list: Vec<Font>,
}

/// One font definition from the Lottie font list.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Font {
    #[serde(default, rename = "fName")]
    /// Lottie font name used by text documents.
    pub name: String,
    #[serde(default, rename = "fFamily")]
    /// Font family name.
    pub family: String,
    #[serde(default, rename = "fStyle")]
    /// Font style name.
    pub style: String,
    #[serde(default)]
    /// Font ascent in Lottie units.
    pub ascent: f32,
}

/// One embedded glyph outline.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct FontCharacter {
    #[serde(default, rename = "ch")]
    /// Grapheme represented by this glyph.
    pub character: String,
    #[serde(default, rename = "size")]
    /// Design size used by the exported glyph.
    pub size: f32,
    #[serde(default, rename = "style")]
    /// Font style associated with the glyph.
    pub style: String,
    #[serde(default, rename = "w")]
    /// Advance width at the design size.
    pub width: f32,
    #[serde(default, rename = "data")]
    /// Vector shapes that describe the glyph outline.
    pub data: FontCharacterData,
    #[serde(default, rename = "fFamily")]
    /// Optional font family override for the glyph.
    pub family: Option<String>,
}

/// Shape data attached to one embedded glyph.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct FontCharacterData {
    #[serde(default, rename = "shapes")]
    /// Shape items that draw the glyph outline.
    pub shapes: Vec<ShapeItem>,
}

/// An image asset or precomposition entry.
#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    #[serde(default, rename = "id")]
    /// Asset identifier referenced by layers.
    pub id: String,
    #[serde(default, rename = "nm")]
    /// Optional asset display name.
    pub name: Option<String>,
    #[serde(default, rename = "w")]
    /// Pixel width for image assets.
    pub width: Option<u32>,
    #[serde(default, rename = "h")]
    /// Pixel height for image assets.
    pub height: Option<u32>,
    #[serde(default, rename = "u")]
    /// Base path for external image assets.
    pub base_path: Option<String>,
    #[serde(default, rename = "p")]
    /// Relative path or embedded data URL for image assets.
    pub path: Option<String>,
    #[serde(default, rename = "e")]
    /// Embedded-image flag from the Lottie schema.
    pub embedded: Option<u8>,
    #[serde(default)]
    /// Child layers when this asset is a precomposition.
    pub layers: Vec<Layer>,
}

/// One Lottie layer.
#[derive(Debug, Clone, Deserialize)]
pub struct Layer {
    #[serde(default, rename = "nm")]
    /// Layer name.
    pub name: String,
    #[serde(rename = "ty")]
    /// Layer type tag.
    pub layer_type: LayerType,
    #[serde(default, rename = "ind")]
    /// Unique layer index used for parenting and matte lookup.
    pub index: Option<i64>,
    #[serde(default, rename = "parent")]
    /// Parent layer index, if any.
    pub parent: Option<i64>,
    #[serde(default, rename = "hd")]
    /// Whether the layer is hidden.
    pub hidden: bool,
    #[serde(default, rename = "ip")]
    /// Layer in-point in frames.
    pub in_point: Option<f32>,
    #[serde(default, rename = "op")]
    /// Layer out-point in frames.
    pub out_point: Option<f32>,
    #[serde(default, rename = "refId")]
    /// Referenced asset identifier for image and precomp layers.
    pub ref_id: Option<String>,
    #[serde(default, rename = "st")]
    /// Layer start time in frames.
    pub start_time: f32,
    #[serde(default = "default_stretch", rename = "sr")]
    /// Playback stretch factor.
    pub stretch: f32,
    #[serde(default, rename = "ks")]
    /// Layer transform block.
    pub transform: Option<Transform>,
    #[serde(default, rename = "tm")]
    /// Optional time-remap property.
    pub time_remap: Option<AnimatedValue>,
    #[serde(default, rename = "shapes")]
    /// Shape list for shape layers.
    pub shapes: Vec<ShapeItem>,
    #[serde(default, rename = "masksProperties")]
    /// Layer masks.
    pub masks: Vec<Mask>,
    #[serde(default, rename = "tt")]
    /// Track matte mode tag.
    pub track_matte: Option<u8>,
    #[serde(default, rename = "tp")]
    /// Explicit matte parent index.
    pub matte_parent: Option<i64>,
    #[serde(default, rename = "td")]
    /// Matte source marker.
    pub matte_source: Option<u8>,
    #[serde(default, rename = "ef")]
    /// Raw layer effect payloads.
    pub effects: Vec<Value>,
    #[serde(default, rename = "t")]
    /// Text payload for text layers.
    pub text: Option<TextData>,
}

/// Numeric Lottie layer type tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct LayerType(#[doc = "Raw Lottie layer type tag."] pub u8);

/// Supported track matte modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TrackMatteMode {
    /// Standard alpha matte.
    Alpha,
    /// Inverted alpha matte.
    AlphaInverted,
    /// Standard luma matte.
    Luma,
    /// Inverted luma matte.
    LumaInverted,
}

/// Text payload attached to a text layer.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TextData {
    #[serde(default, rename = "d")]
    /// Animated text document collection.
    pub document: TextDocumentCollection,
    #[serde(default, rename = "a")]
    /// Raw text animator definitions.
    pub animators: Vec<Value>,
    #[serde(default, rename = "p")]
    /// Raw text path definition.
    pub path: Value,
}

/// Animated collection of text documents.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TextDocumentCollection {
    #[serde(default, rename = "k")]
    /// Text document keyframes.
    pub keyframes: Vec<TextDocumentKeyframe>,
}

/// One keyframed text document.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TextDocumentKeyframe {
    #[serde(default, rename = "s")]
    /// Text document value.
    pub document: TextDocument,
    #[serde(default, rename = "t")]
    /// Keyframe time in frames.
    pub time: f32,
}

/// Resolved text styling and layout data.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TextDocument {
    #[serde(default, rename = "t")]
    /// Text content.
    pub text: String,
    #[serde(default, rename = "f")]
    /// Font name.
    pub font: String,
    #[serde(default, rename = "s")]
    /// Font size.
    pub size: f32,
    #[serde(default, rename = "j")]
    /// Justification mode.
    pub justify: u8,
    #[serde(default, rename = "tr")]
    /// Tracking adjustment.
    pub tracking: f32,
    #[serde(default, rename = "lh")]
    /// Line height.
    pub line_height: f32,
    #[serde(default, rename = "ls")]
    /// Baseline shift.
    pub baseline_shift: f32,
    #[serde(default, rename = "fc")]
    /// Fill color as normalized RGB or RGBA components.
    pub fill_color: Vec<f32>,
    #[serde(default, rename = "sc")]
    /// Stroke color as normalized RGB or RGBA components.
    pub stroke_color: Vec<f32>,
    #[serde(default, rename = "sw")]
    /// Stroke width.
    pub stroke_width: f32,
    #[serde(default, rename = "ps")]
    /// Text box position.
    pub position: Option<[f32; 2]>,
    #[serde(default, rename = "sz")]
    /// Text box size.
    pub box_size: Option<[f32; 2]>,
}

/// Standard Lottie transform block.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Transform {
    #[serde(default, rename = "a")]
    /// Anchor point.
    pub anchor: Option<AnimatedValue>,
    #[serde(default, rename = "p")]
    /// Position property.
    pub position: Option<PositionValue>,
    #[serde(default, rename = "s")]
    /// Scale property.
    pub scale: Option<AnimatedValue>,
    #[serde(default, rename = "r")]
    /// Rotation property.
    pub rotation: Option<AnimatedValue>,
    #[serde(default, rename = "o")]
    /// Opacity property.
    pub opacity: Option<AnimatedValue>,
    #[serde(default, rename = "sk")]
    /// Skew property.
    pub skew: Option<AnimatedValue>,
    #[serde(default, rename = "sa")]
    /// Skew-axis property.
    pub skew_axis: Option<AnimatedValue>,
}

/// One layer mask definition.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Mask {
    #[serde(default, rename = "mode")]
    /// Raw mask mode tag.
    pub mode: Option<String>,
    #[serde(default, rename = "pt")]
    /// Mask path.
    pub path: Option<ShapePathValue>,
    #[serde(default, rename = "o")]
    /// Mask opacity.
    pub opacity: Option<AnimatedValue>,
    #[serde(default, rename = "inv")]
    /// Whether the mask is inverted.
    pub inverted: bool,
}

/// Supported mask modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MaskMode {
    /// Additive mask.
    Add,
    /// Subtractive mask.
    Subtract,
    /// Intersection mask.
    Intersect,
    /// Disabled mask.
    None,
}

/// Generic scalar or vector animated property.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct AnimatedValue {
    #[serde(default, rename = "a")]
    /// Lottie animated flag.
    pub animated: Option<u8>,
    #[serde(default, rename = "k")]
    /// Raw keyframe payload.
    pub keyframes: Option<Value>,
    #[serde(default, rename = "x")]
    /// Optional expression payload.
    pub expression: Option<Value>,
}

/// Position property that may be stored as one combined vector or split axes.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum PositionValue {
    /// Split-axis position.
    Split(SplitPosition),
    /// Combined vector position.
    Combined(AnimatedValue),
}

/// Split-axis position property.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SplitPosition {
    #[serde(default, rename = "s")]
    /// Split-position flag.
    pub split: Option<u8>,
    #[serde(default, rename = "x")]
    /// X component.
    pub x: Option<AnimatedValue>,
    #[serde(default, rename = "y")]
    /// Y component.
    pub y: Option<AnimatedValue>,
    #[serde(default, rename = "z")]
    /// Optional Z component.
    pub z: Option<AnimatedValue>,
}

/// A static cubic Bezier path.
#[derive(Debug, Clone, PartialEq)]
pub struct BezierPath {
    /// Whether the path is closed.
    pub closed: bool,
    /// Ordered vertices with relative tangents.
    pub vertices: Vec<BezierVertex>,
}

/// One cubic Bezier vertex.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BezierVertex {
    /// Anchor point.
    pub vertex: [f32; 2],
    /// Incoming tangent, stored relative to `vertex`.
    pub in_tangent: [f32; 2],
    /// Outgoing tangent, stored relative to `vertex`.
    pub out_tangent: [f32; 2],
}

/// Animated property that stores a shape path.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ShapePathValue {
    #[serde(default, rename = "a")]
    /// Lottie animated flag.
    pub animated: Option<u8>,
    #[serde(default, rename = "k")]
    /// Raw keyframe payload.
    pub keyframes: Option<Value>,
    #[serde(default, rename = "x")]
    /// Optional expression payload.
    pub expression: Option<Value>,
}

/// Generic Lottie shape item.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ShapeItem {
    #[serde(default, rename = "nm")]
    /// Item name.
    pub name: String,
    #[serde(default, rename = "ty")]
    /// Shape item type tag.
    pub item_type: String,
    #[serde(default, rename = "it")]
    /// Nested shape items for groups.
    pub items: Vec<Self>,
    #[serde(default, rename = "hd")]
    /// Whether the item is hidden.
    pub hidden: bool,
    #[serde(default, rename = "x")]
    /// Optional expression payload.
    pub expression: Option<Value>,
    #[serde(default, rename = "ks")]
    /// Path property for `sh` items.
    pub path: Option<ShapePathValue>,
    #[serde(default, rename = "p")]
    /// Position-like property used by several item kinds.
    pub position: Option<PositionValue>,
    #[serde(default, rename = "s")]
    /// Size-like property used by several item kinds.
    pub size: Option<AnimatedValue>,
    #[serde(default, rename = "c")]
    /// Color-like property used by several item kinds.
    pub color: Option<AnimatedValue>,
    #[serde(default, rename = "o")]
    /// Opacity-like property used by several item kinds.
    pub opacity: Option<AnimatedValue>,
    #[serde(default, rename = "w")]
    /// Width-like property used by several item kinds.
    pub width: Option<AnimatedValue>,
    #[serde(default, rename = "a")]
    /// Anchor-like property used by several item kinds.
    pub anchor: Option<AnimatedValue>,
    #[serde(default, rename = "sk")]
    /// Skew property for transform items.
    pub skew: Option<AnimatedValue>,
    #[serde(default, rename = "sa")]
    /// Skew-axis property for transform items.
    pub skew_axis: Option<AnimatedValue>,
    #[serde(default, rename = "lc")]
    /// Stroke line-cap tag.
    pub line_cap: Option<u8>,
    #[serde(default, rename = "lj")]
    /// Stroke line-join tag.
    pub line_join: Option<u8>,
    #[serde(default, rename = "ml")]
    /// Static miter limit.
    pub miter_limit: Option<f32>,
    #[serde(default, rename = "ml2")]
    /// Animated miter limit.
    pub miter_limit_value: Option<AnimatedValue>,
    #[serde(flatten)]
    /// Unmodeled per-item fields retained in raw JSON form.
    pub extra: BTreeMap<String, Value>,
}

/// Gradient definition attached to a fill or stroke item.
#[derive(Debug, Clone, Deserialize)]
pub struct GradientData {
    #[serde(rename = "p")]
    /// Number of color stops.
    pub point_count: usize,
    #[serde(rename = "k")]
    /// Encoded color-stop data.
    pub colors: AnimatedValue,
}

/// Dash pattern entry for a stroked shape.
#[derive(Debug, Clone, Deserialize)]
pub struct DashPatternEntry {
    #[serde(default, rename = "n")]
    /// Entry kind, such as dash, gap, or offset.
    pub name: String,
    #[serde(default, rename = "v")]
    /// Entry value.
    pub value: AnimatedValue,
}

/// Transform block used by repeater items.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RepeaterTransform {
    #[serde(default, rename = "a")]
    /// Anchor point.
    pub anchor: Option<AnimatedValue>,
    #[serde(default, rename = "p")]
    /// Position property.
    pub position: Option<PositionValue>,
    #[serde(default, rename = "s")]
    /// Scale property.
    pub scale: Option<AnimatedValue>,
    #[serde(default, rename = "r")]
    /// Rotation property.
    pub rotation: Option<AnimatedValue>,
    #[serde(default, rename = "o")]
    /// Overall opacity property.
    pub opacity: Option<AnimatedValue>,
    #[serde(default, rename = "so")]
    /// Opacity at the start of the repeated range.
    pub start_opacity: Option<AnimatedValue>,
    #[serde(default, rename = "eo")]
    /// Opacity at the end of the repeated range.
    pub end_opacity: Option<AnimatedValue>,
}

const fn default_stretch() -> f32 {
    1.0
}

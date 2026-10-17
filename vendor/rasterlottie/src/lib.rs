#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[macro_use]
mod trace_macros;

#[cfg(feature = "dotlottie")]
mod dotlottie;
mod effects;
mod error;
mod expression;
mod model;
mod model_parse;
mod model_types;
mod render;
mod support;
mod timeline;

pub use error::RasterlottieError;
pub use model::{
    AnimatedValue, Animation, Asset, BezierPath, BezierVertex, Font, FontCharacter, Layer,
    LayerType, MaskMode, PositionValue, ShapeItem, ShapePathValue, SplitPosition, TextData,
    TextDocument, TrackMatteMode, Transform,
};
#[cfg(feature = "gif")]
pub use render::GifRenderConfig;
pub use render::{
    ImageAssetResolver, Pixmap, PreparedAnimation, RasterFrame, RenderConfig, Renderer, Rgba8,
};
pub use support::{
    SupportProfile, SupportReport, UnsupportedFeature, UnsupportedKind, analyze_animation,
    analyze_animation_with_profile,
};

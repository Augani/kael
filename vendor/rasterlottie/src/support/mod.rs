//! Support analysis module declarations and public exports.

mod analyzer;
mod analyzer_shapes;
mod analyzer_text;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_advanced;

pub use analyzer::{
    SupportProfile, SupportReport, UnsupportedFeature, UnsupportedKind, analyze_animation,
    analyze_animation_with_profile,
};

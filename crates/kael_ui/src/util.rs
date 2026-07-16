//! # Utility Extensions and Helper Functions
//!
//! Common extension traits and utility functions that enhance Kael's standard types
//! with convenient methods used throughout the component library.
//! ## Extension Traits
//!
//! - **`AxisExt`**: Convenient methods for checking axis orientation
//! - **`PixelsExt`**: Conversion utilities for Pixels type
//! - **`ScrollHandleOffsetable`**: Trait for scroll handle offset operations
//! - **Color utilities**: Color manipulation and conversion helpers
//! - **Layout helpers**: Common layout calculations and measurements
//!
//! ## Design Decisions
//!
//! - **Minimal Surface Area**: Only essential utilities that are reused across components
//! - **Type Safety**: Extension traits maintain strong typing and prevent errors
//! - **Performance**: Zero-cost abstractions that compile to efficient machine code
//! - **Consistency**: Standardized patterns for common operations
//! - **Discoverability**: Clear naming that makes functionality obvious
//!

use kael::{
    AccessibilityAction, AccessibilityActionPayload, AccessibilityActionRequest, Pixels, Point,
    ScrollHandle, Size,
};

/// Resolve an assistive-technology adjustment into a finite, bounded numeric value.
pub(crate) fn accessibility_adjusted_value(
    request: &AccessibilityActionRequest,
    current: f64,
    minimum: f64,
    maximum: f64,
    step: f64,
) -> Option<f64> {
    if !current.is_finite()
        || !minimum.is_finite()
        || !maximum.is_finite()
        || !step.is_finite()
        || step <= 0.0
    {
        return None;
    }
    let (minimum, maximum) = if minimum <= maximum {
        (minimum, maximum)
    } else {
        (maximum, minimum)
    };
    let requested = match request.action {
        AccessibilityAction::Increment => current + step,
        AccessibilityAction::Decrement => current - step,
        AccessibilityAction::SetValue => match request.payload.as_ref()? {
            AccessibilityActionPayload::NumericValue(value) => *value,
            AccessibilityActionPayload::Value(value) => value.parse().ok()?,
        },
        _ => return None,
    };
    requested
        .is_finite()
        .then(|| requested.clamp(minimum, maximum))
}

/// Extension trait for Axis
pub trait AxisExt {
    /// Returns true if the axis is horizontal
    #[allow(clippy::wrong_self_convention)]
    fn is_horizontal(self) -> bool;
    /// Returns true if the axis is vertical
    #[allow(clippy::wrong_self_convention)]
    fn is_vertical(self) -> bool;
}

impl AxisExt for kael::Axis {
    fn is_horizontal(self) -> bool {
        self == kael::Axis::Horizontal
    }

    fn is_vertical(self) -> bool {
        self == kael::Axis::Vertical
    }
}

/// Extension trait for converting Pixels to f32 and f64
pub trait PixelsExt {
    /// Convert to f32
    fn as_f32(&self) -> f32;
    /// Convert to f64
    #[allow(clippy::wrong_self_convention)]
    fn as_f64(self) -> f64;
}

impl PixelsExt for Pixels {
    fn as_f32(&self) -> f32 {
        f32::from(*self)
    }

    fn as_f64(self) -> f64 {
        f64::from(self)
    }
}

/// Trait for types that can be used as scroll handles with offset tracking
pub trait ScrollHandleOffsetable {
    /// Get the current scroll offset
    fn offset(&self) -> Point<Pixels>;
    /// Set the scroll offset
    fn set_offset(&self, offset: Point<Pixels>);
    /// Get the full content size
    fn content_size(&self) -> Size<Pixels>;
}

impl ScrollHandleOffsetable for ScrollHandle {
    fn offset(&self) -> Point<Pixels> {
        self.offset()
    }

    fn set_offset(&self, offset: Point<Pixels>) {
        self.set_offset(offset);
    }

    fn content_size(&self) -> Size<Pixels> {
        self.max_offset() + self.bounds().size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kael::AccessibilityRole;

    fn request(
        action: AccessibilityAction,
        payload: Option<AccessibilityActionPayload>,
    ) -> AccessibilityActionRequest {
        let id = kael::AccessibilityNode::new(AccessibilityRole::Slider).id;
        let mut request = AccessibilityActionRequest::new(id, action);
        request.payload = payload;
        request
    }

    #[test]
    fn accessibility_adjustments_parse_and_clamp_slider_values() {
        assert_eq!(
            accessibility_adjusted_value(
                &request(AccessibilityAction::Increment, None),
                0.9,
                0.0,
                1.0,
                0.2,
            ),
            Some(1.0)
        );
        assert_eq!(
            accessibility_adjusted_value(
                &request(
                    AccessibilityAction::SetValue,
                    Some(AccessibilityActionPayload::Value("0.42".into())),
                ),
                0.0,
                0.0,
                1.0,
                0.05,
            ),
            Some(0.42)
        );
        assert_eq!(
            accessibility_adjusted_value(
                &request(
                    AccessibilityAction::SetValue,
                    Some(AccessibilityActionPayload::NumericValue(f64::NAN)),
                ),
                0.0,
                0.0,
                1.0,
                0.05,
            ),
            None
        );
    }
}

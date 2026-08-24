//! Shared, deterministic conversions for native high-fidelity pointer backends.

/// Normalize an unsigned device value to a finite inclusive `0..=1` range.
#[cfg(any(test, target_os = "windows"))]
pub(crate) fn normalize_unsigned(value: u32, maximum: u32) -> f32 {
    if maximum == 0 {
        return 0.0;
    }
    (value as f64 / maximum as f64).clamp(0.0, 1.0) as f32
}

/// Clamp a native floating-point pressure value without propagating NaNs.
#[cfg(any(test, target_os = "macos"))]
pub(crate) fn clamp_pressure(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Clamp a native floating-point tangential pressure value without propagating NaNs.
#[cfg(any(test, target_os = "macos"))]
pub(crate) fn clamp_tangential_pressure(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// Convert AppKit's normalized tablet tilt axis to Pointer Events degrees.
#[cfg(any(test, target_os = "macos"))]
pub(crate) fn normalized_tilt_to_degrees(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-1.0, 1.0).asin().to_degrees()
    } else {
        0.0
    }
}

/// Normalize native rotation into Pointer Events' clockwise `0..<360` domain.
#[cfg(any(test, target_os = "macos", target_os = "windows"))]
pub(crate) fn normalize_twist_degrees(value: f32) -> f32 {
    if value.is_finite() {
        value.rem_euclid(360.0)
    } else {
        0.0
    }
}

/// Return a finite non-negative logical contact extent.
#[cfg(any(test, target_os = "windows"))]
pub(crate) fn contact_extent(physical_extent: i32, scale_factor: f32) -> f32 {
    if !scale_factor.is_finite() || scale_factor <= 0.0 {
        return 0.0;
    }
    physical_extent.max(0) as f32 / scale_factor
}

/// Convert Wayland's oriented major/minor contact ellipse to axis-aligned extents.
#[cfg(any(
    test,
    all(any(target_os = "linux", target_os = "freebsd"), feature = "wayland")
))]
pub(crate) fn oriented_contact_size(
    major: f32,
    minor: f32,
    orientation_degrees: f32,
) -> (f32, f32) {
    if !major.is_finite()
        || !minor.is_finite()
        || !orientation_degrees.is_finite()
        || major < 0.0
        || minor < 0.0
    {
        return (0.0, 0.0);
    }
    let radians = orientation_degrees.to_radians();
    let sin = radians.sin();
    let cos = radians.cos();
    // Wayland measures the major axis clockwise from positive surface Y.
    let width = ((major * sin).powi(2) + (minor * cos).powi(2)).sqrt();
    let height = ((major * cos).powi(2) + (minor * sin).powi(2)).sqrt();
    (width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsigned_pressure_is_bounded_and_handles_empty_ranges() {
        assert_eq!(normalize_unsigned(0, 1024), 0.0);
        assert_eq!(normalize_unsigned(512, 1024), 0.5);
        assert_eq!(normalize_unsigned(2048, 1024), 1.0);
        assert_eq!(normalize_unsigned(10, 0), 0.0);
    }

    #[test]
    fn native_floats_never_publish_nan_or_out_of_range_values() {
        assert_eq!(clamp_pressure(f32::NAN), 0.0);
        assert_eq!(clamp_pressure(-1.0), 0.0);
        assert_eq!(clamp_pressure(2.0), 1.0);
        assert_eq!(clamp_tangential_pressure(f32::INFINITY), 0.0);
        assert_eq!(clamp_tangential_pressure(-2.0), -1.0);
        assert_eq!(clamp_tangential_pressure(2.0), 1.0);
    }

    #[test]
    fn appkit_tilt_conversion_matches_pointer_event_units() {
        assert!((normalized_tilt_to_degrees(0.5) - 30.0).abs() < 0.001);
        assert_eq!(normalized_tilt_to_degrees(1.0), 90.0);
        assert_eq!(normalized_tilt_to_degrees(-1.0), -90.0);
        assert_eq!(normalized_tilt_to_degrees(f32::NAN), 0.0);
    }

    #[test]
    fn twist_and_contact_geometry_are_sanitized() {
        assert_eq!(normalize_twist_degrees(-10.0), 350.0);
        assert_eq!(normalize_twist_degrees(370.0), 10.0);
        assert_eq!(normalize_twist_degrees(f32::NAN), 0.0);
        assert_eq!(contact_extent(25, 2.0), 12.5);
        assert_eq!(contact_extent(-25, 2.0), 0.0);
        assert_eq!(contact_extent(25, 0.0), 0.0);
        assert_eq!(oriented_contact_size(20.0, 10.0, 0.0), (10.0, 20.0));
        let rotated = oriented_contact_size(20.0, 10.0, 90.0);
        assert!((rotated.0 - 20.0).abs() < 0.001);
        assert!((rotated.1 - 10.0).abs() < 0.001);
        assert_eq!(oriented_contact_size(f32::NAN, 10.0, 0.0), (0.0, 0.0));
    }
}

// Feature: platform-parity-electron-features, Property 10: Session state serialization round-trip

use proptest::prelude::*;

use crate::{DisplayId, WindowBounds, WindowState};

/// Generates an arbitrary `WindowBounds` value.
fn window_bounds_strategy() -> impl Strategy<Value = WindowBounds> {
    prop_oneof![
        (
            0.0f32..1920.0f32,
            0.0f32..1080.0f32,
            100.0f32..800.0f32,
            100.0f32..600.0f32
        )
            .prop_map(|(x, y, w, h)| WindowBounds::Windowed(crate::Bounds::new(
                crate::point(crate::px(x), crate::px(y)),
                crate::size(crate::px(w), crate::px(h)),
            ))),
        (
            0.0f32..1920.0f32,
            0.0f32..1080.0f32,
            100.0f32..800.0f32,
            100.0f32..600.0f32
        )
            .prop_map(|(x, y, w, h)| WindowBounds::Maximized(crate::Bounds::new(
                crate::point(crate::px(x), crate::px(y)),
                crate::size(crate::px(w), crate::px(h)),
            ))),
    ]
}

/// Generates an arbitrary `WindowState`.
fn window_state_strategy() -> impl Strategy<Value = WindowState> {
    (
        window_bounds_strategy(),
        prop::option::of(any::<u32>()),
        prop::bool::ANY,
    )
        .prop_map(|(bounds, display_id, fullscreen)| WindowState {
            bounds,
            display_id: display_id.map(DisplayId),
            fullscreen,
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 18.1**
    ///
    /// For any valid `WindowState`, serializing to JSON and deserializing
    /// produces an equivalent value.
    #[test]
    fn session_state_serialization_roundtrip(state in window_state_strategy()) {
        let json = serde_json::to_string(&state).expect("WindowState should serialize to JSON");
        let deserialized: WindowState = serde_json::from_str(&json)
            .expect("WindowState should deserialize from JSON");

        prop_assert_eq!(state.bounds.get_bounds().origin.x.0, deserialized.bounds.get_bounds().origin.x.0);
        prop_assert_eq!(state.bounds.get_bounds().origin.y.0, deserialized.bounds.get_bounds().origin.y.0);
        prop_assert_eq!(state.bounds.get_bounds().size.width.0, deserialized.bounds.get_bounds().size.width.0);
        prop_assert_eq!(state.bounds.get_bounds().size.height.0, deserialized.bounds.get_bounds().size.height.0);
        prop_assert_eq!(state.display_id, deserialized.display_id);
        prop_assert_eq!(state.fullscreen, deserialized.fullscreen);
    }
}

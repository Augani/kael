use super::super::{
    tests::{assert_fixture_supported, color_sum, load_fixture_animation, pixel_at},
    *,
};
use crate::Animation;

#[test]
fn fixture_linear_gradient_opacity_stops_behave_as_expected() {
    let animation = load_fixture_animation("gradient_linear_opacity_stops.json");
    assert_fixture_supported(&animation);

    let frame = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();

    let left = pixel_at(&frame, 25, 50);
    let middle = pixel_at(&frame, 50, 50);
    let right = pixel_at(&frame, 75, 50);

    assert!(left[0] > left[2], "left pixel should skew red: {left:?}");
    assert!(
        right[2] > right[0],
        "right pixel should skew blue: {right:?}"
    );
    assert!(
        left[3] < middle[3] && middle[3] < right[3],
        "opacity stops should increase alpha across the gradient: left={left:?} middle={middle:?} right={right:?}"
    );
}

#[test]
fn fixture_radial_gradient_highlight_offsets_the_focus() {
    let animation = load_fixture_animation("gradient_radial_highlight_angle.json");
    assert_fixture_supported(&animation);

    let frame = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();

    let upper = pixel_at(&frame, 50, 34);
    let lower = pixel_at(&frame, 50, 66);
    let edge = pixel_at(&frame, 78, 50);

    assert!(
        color_sum(lower) > color_sum(upper),
        "highlight angle should bias the bright area downward: upper={upper:?} lower={lower:?}"
    );
    assert!(
        color_sum(edge) < color_sum(lower),
        "edge should remain darker than the shifted highlight: edge={edge:?} lower={lower:?}"
    );
}

#[test]
fn fixture_subtract_mask_cuts_a_hole() {
    let animation = load_fixture_animation("mask_subtract_hole.json");
    assert_fixture_supported(&animation);

    let frame = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&frame, 25, 50), [255, 0, 0, 255]);
    assert_eq!(pixel_at(&frame, 50, 50), [0, 0, 0, 0]);
    assert_eq!(pixel_at(&frame, 75, 50), [255, 0, 0, 255]);
}

#[test]
fn fixture_track_matte_prefers_explicit_parent_over_index_fallback() {
    let animation = load_fixture_animation("track_matte_parent_redirect.json");
    assert_fixture_supported(&animation);

    let frame = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&frame, 30, 50), [0, 255, 0, 255]);
    assert_eq!(pixel_at(&frame, 70, 50), [0, 0, 0, 0]);
}

#[test]
fn fixture_layer_parenting_applies_parent_transform() {
    let animation = load_fixture_animation("layer_parenting_basic.json");
    assert_fixture_supported(&animation);

    let frame = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&frame, 20, 50), [0, 0, 0, 0]);
    assert_eq!(pixel_at(&frame, 40, 50), [0, 255, 0, 255]);
}

#[test]
fn parent_opacity_does_not_hide_child_layers() {
    let animation = Animation::from_json_str(
        r#"{
                "v":"5.7.6",
                "fr":30,
                "ip":0,
                "op":60,
                "w":64,
                "h":64,
                "layers":[
                    {
                        "nm":"Controller",
                        "ind":1,
                        "ty":3,
                        "ks":{
                            "a":{"a":0,"k":[0,0]},
                            "p":{"a":0,"k":[16,0]},
                            "s":{"a":0,"k":[100,100]},
                            "r":{"a":0,"k":0},
                            "o":{"a":0,"k":0}
                        }
                    },
                    {
                        "nm":"Child",
                        "ind":2,
                        "parent":1,
                        "ty":4,
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[16,32]},"s":{"a":0,"k":[16,16]},"r":{"a":0,"k":0}},
                                    {"ty":"fl","c":{"a":0,"k":[0,1,0,1]},"o":{"a":0,"k":100}},
                                    {"ty":"tr","a":{"a":0,"k":[0,0]},"p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}
                                ]
                            }
                        ]
                    }
                ]
            }"#,
    )
    .unwrap();
    assert_fixture_supported(&animation);

    let frame = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&frame, 16, 32), [0, 0, 0, 0]);
    assert_eq!(pixel_at(&frame, 32, 32), [0, 255, 0, 255]);
}

#[test]
fn fixture_stroke_dash_creates_gaps() {
    let animation = load_fixture_animation("stroke_dash_basic.json");
    assert_fixture_supported(&animation);

    let frame = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&frame, 15, 50), [255, 255, 255, 255]);
    assert_eq!(pixel_at(&frame, 25, 50), [0, 0, 0, 0]);
    assert_eq!(pixel_at(&frame, 35, 50), [255, 255, 255, 255]);
}

#[test]
fn fixture_trim_path_limits_the_visible_segment() {
    let animation = load_fixture_animation("trim_path_basic.json");
    assert_fixture_supported(&animation);

    let frame = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&frame, 15, 50), [0, 0, 0, 0]);
    assert_eq!(pixel_at(&frame, 50, 50), [255, 0, 0, 255]);
    assert_eq!(pixel_at(&frame, 85, 50), [0, 0, 0, 0]);
}

#[test]
fn fixture_polystar_draws_a_filled_star() {
    let animation = load_fixture_animation("polystar_basic.json");
    assert_fixture_supported(&animation);

    let frame = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&frame, 50, 50), [255, 255, 0, 255]);
    assert_eq!(pixel_at(&frame, 10, 10), [0, 0, 0, 0]);
}

#[test]
fn fixture_repeater_duplicates_the_source_geometry() {
    let animation = load_fixture_animation("repeater_basic.json");
    assert_fixture_supported(&animation);

    let frame = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&frame, 20, 50), [0, 0, 255, 255]);
    assert_eq!(pixel_at(&frame, 40, 50), [0, 0, 255, 255]);
    assert_eq!(pixel_at(&frame, 60, 50), [0, 0, 255, 255]);
    assert_eq!(pixel_at(&frame, 80, 50), [0, 0, 0, 0]);
}

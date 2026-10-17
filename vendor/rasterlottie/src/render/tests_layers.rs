#![cfg(test)]

#[cfg(feature = "images")]
use super::tests::{solid_png_bytes, solid_png_data_url};
use super::{tests::pixel_at, *};
use crate::Animation;
#[cfg(feature = "images")]
use crate::RasterlottieError;
#[cfg(any(feature = "images", feature = "text"))]
use crate::analyze_animation;

#[cfg(feature = "images")]
#[test]
fn renderer_draws_embedded_image_layers() {
    let data_url = solid_png_data_url([255, 0, 0, 255]);
    let animation = Animation::from_json_str(&format!(
        r#"{{
                "v":"5.7.6",
                "fr":30,
                "ip":0,
                "op":60,
                "w":16,
                "h":16,
                "assets":[{{"id":"img","w":16,"h":16,"p":"{data_url}","e":1}}],
                "layers":[
                    {{
                        "nm":"Image",
                        "ind":1,
                        "ty":2,
                        "refId":"img",
                        "ks":{{
                            "a":{{"a":0,"k":[0,0]}},
                            "p":{{"a":0,"k":[0,0]}},
                            "s":{{"a":0,"k":[100,100]}},
                            "r":{{"a":0,"k":0}},
                            "o":{{"a":0,"k":100}}
                        }}
                    }}
                ]
            }}"#
    ))
    .unwrap();

    assert!(analyze_animation(&animation).is_supported());

    let frame = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&frame, 8, 8), [255, 0, 0, 255]);
}

#[cfg(feature = "images")]
#[test]
fn renderer_draws_external_image_layers_with_a_resolver() {
    let animation = Animation::from_json_str(
        r#"{
                "v":"5.7.6",
                "fr":30,
                "ip":0,
                "op":60,
                "w":16,
                "h":16,
                "assets":[{"id":"img","w":16,"h":16,"u":"images/","p":"cat.png"}],
                "layers":[
                    {
                        "nm":"Image",
                        "ind":1,
                        "ty":2,
                        "refId":"img",
                        "ks":{
                            "a":{"a":0,"k":[0,0]},
                            "p":{"a":0,"k":[0,0]},
                            "s":{"a":0,"k":[100,100]},
                            "r":{"a":0,"k":0},
                            "o":{"a":0,"k":100}
                        }
                    }
                ]
            }"#,
    )
    .unwrap();

    let frame = Renderer::default()
        .render_frame_with_resolver(
            &animation,
            0.0,
            RenderConfig::default(),
            &|asset: &crate::Asset| {
                assert_eq!(asset.path.as_deref(), Some("cat.png"));
                Ok(Some(solid_png_bytes([255, 0, 0, 255])))
            },
        )
        .unwrap();

    assert_eq!(pixel_at(&frame, 8, 8), [255, 0, 0, 255]);
}

#[cfg(feature = "images")]
#[test]
fn renderer_ignores_invalid_embedded_image_assets_until_they_are_used() {
    let animation = Animation::from_json_str(
        r#"{
                "v":"5.7.6",
                "fr":30,
                "ip":0,
                "op":60,
                "w":16,
                "h":16,
                "assets":[{"id":"img","w":16,"h":16,"p":"data:image/png;base64,not-valid","e":1}],
                "layers":[
                    {
                        "nm":"Shape",
                        "ind":1,
                        "ty":4,
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[8,8]},"s":{"a":0,"k":[8,8]},"r":{"a":0,"k":0}},
                                    {"ty":"fl","c":{"a":0,"k":[1,0,0,1]},"o":{"a":0,"k":100}},
                                    {"ty":"tr","a":{"a":0,"k":[0,0]},"p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}
                                ]
                            }
                        ]
                    }
                ]
            }"#,
    )
    .unwrap();

    let prepared = Renderer::default().prepare(&animation).unwrap();
    let frame = prepared.render_frame(0.0, RenderConfig::default()).unwrap();

    assert_eq!(pixel_at(&frame, 8, 8), [255, 0, 0, 255]);
}

#[cfg(feature = "images")]
#[test]
fn renderer_reports_invalid_embedded_image_assets_when_they_are_used() {
    let animation = Animation::from_json_str(
        r#"{
                "v":"5.7.6",
                "fr":30,
                "ip":0,
                "op":60,
                "w":16,
                "h":16,
                "assets":[{"id":"img","w":16,"h":16,"p":"data:image/png;base64,not-valid","e":1}],
                "layers":[
                    {
                        "nm":"Image",
                        "ind":1,
                        "ty":2,
                        "refId":"img",
                        "ks":{
                            "a":{"a":0,"k":[0,0]},
                            "p":{"a":0,"k":[0,0]},
                            "s":{"a":0,"k":[100,100]},
                            "r":{"a":0,"k":0},
                            "o":{"a":0,"k":100}
                        }
                    }
                ]
            }"#,
    )
    .unwrap();

    let error = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap_err();

    assert!(matches!(error, RasterlottieError::InvalidImageAsset { .. }));
}

#[cfg(feature = "text")]
#[test]
fn renderer_draws_glyph_text_layers() {
    let animation = Animation::from_json_str(
        r#"{
                "v":"5.7.6",
                "fr":30,
                "ip":0,
                "op":60,
                "w":128,
                "h":128,
                "fonts":{"list":[{"fName":"TestFont","fFamily":"Test Family","fStyle":"Regular","ascent":75}]},
                "chars":[
                    {
                        "ch":"A",
                        "size":100,
                        "style":"Regular",
                        "w":60,
                        "fFamily":"Test Family",
                        "data":{
                            "shapes":[
                                {
                                    "ty":"gr",
                                    "it":[
                                        {"ty":"sh","ks":{"a":0,"k":{"c":true,"i":[[0,0],[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0],[0,0]],"v":[[0,-80],[60,-80],[60,0],[0,0]]}}}
                                    ]
                                }
                            ]
                        }
                    }
                ],
                "layers":[
                    {
                        "nm":"Text",
                        "ind":1,
                        "ty":5,
                        "t":{
                            "d":{"k":[{"s":{"s":100,"f":"TestFont","t":"A","j":0,"tr":0,"lh":120,"ls":0,"fc":[1,0,0]},"t":0}]},
                            "p":{},
                            "m":{"g":1},
                            "a":[]
                        }
                    }
                ]
            }"#,
    )
    .unwrap();

    assert!(analyze_animation(&animation).is_supported());

    let frame = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&frame, 30, 40), [255, 0, 0, 255]);
    assert_eq!(pixel_at(&frame, 90, 40), [0, 0, 0, 0]);
}

#[test]
fn renderer_applies_an_add_mask() {
    let animation = Animation::from_json_str(
        r#"{
                "v":"5.7.6",
                "fr":30,
                "ip":0,
                "op":60,
                "w":100,
                "h":100,
                "layers":[
                    {
                        "nm":"Masked Layer",
                        "ind":1,
                        "ty":4,
                        "masksProperties":[
                            {
                                "mode":"a",
                                "pt":{"a":0,"k":{"c":true,"i":[[0,0],[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0],[0,0]],"v":[[30,30],[70,30],[70,70],[30,70]]}},
                                "o":{"a":0,"k":100}
                            }
                        ],
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[50,50]},"s":{"a":0,"k":[80,80]},"r":{"a":0,"k":0}},
                                    {"ty":"fl","c":{"a":0,"k":[1,0,0,1]},"o":{"a":0,"k":100}},
                                    {"ty":"tr","a":{"a":0,"k":[0,0]},"p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}
                                ]
                            }
                        ]
                    }
                ]
            }"#,
    )
    .unwrap();

    let frame = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&frame, 50, 50), [255, 0, 0, 255]);
    assert_eq!(pixel_at(&frame, 20, 20), [0, 0, 0, 0]);
}

#[test]
fn renderer_applies_a_hidden_alpha_matte() {
    let animation = Animation::from_json_str(
        r#"{
                "v":"5.7.6",
                "fr":30,
                "ip":0,
                "op":60,
                "w":100,
                "h":100,
                "layers":[
                    {
                        "nm":"Matte Source",
                        "ind":1,
                        "ty":4,
                        "hd":true,
                        "td":1,
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[50,50]},"s":{"a":0,"k":[40,40]},"r":{"a":0,"k":0}},
                                    {"ty":"fl","c":{"a":0,"k":[1,1,1,1]},"o":{"a":0,"k":100}},
                                    {"ty":"tr","a":{"a":0,"k":[0,0]},"p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}
                                ]
                            }
                        ]
                    },
                    {
                        "nm":"Target",
                        "ind":2,
                        "ty":4,
                        "tt":1,
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[50,50]},"s":{"a":0,"k":[80,80]},"r":{"a":0,"k":0}},
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

    let frame = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&frame, 50, 50), [0, 255, 0, 255]);
    assert_eq!(pixel_at(&frame, 20, 20), [0, 0, 0, 0]);
}

#[test]
fn renderer_fills_a_linear_gradient_rectangle() {
    let animation = Animation::from_json_str(
        r#"{
                "v":"5.7.6",
                "fr":30,
                "ip":0,
                "op":60,
                "w":100,
                "h":100,
                "layers":[
                    {
                        "nm":"Gradient Layer",
                        "ind":1,
                        "ty":4,
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[50,50]},"s":{"a":0,"k":[60,60]},"r":{"a":0,"k":0}},
                                    {"ty":"gf","o":{"a":0,"k":100},"s":{"a":0,"k":[20,50]},"e":{"a":0,"k":[80,50]},"t":1,"g":{"p":2,"k":{"a":0,"k":[0,1,0,0,1,0,0,1]}}},
                                    {"ty":"tr","a":{"a":0,"k":[0,0]},"p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}
                                ]
                            }
                        ]
                    }
                ]
            }"#,
    )
    .unwrap();

    let frame = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();

    let left = pixel_at(&frame, 25, 50);
    let right = pixel_at(&frame, 75, 50);
    let middle = pixel_at(&frame, 50, 50);

    assert!(left[0] > left[2], "left pixel should skew red: {left:?}");
    assert!(
        right[2] > right[0],
        "right pixel should skew blue: {right:?}"
    );
    assert!(
        middle[0] > 60 && middle[2] > 60,
        "middle pixel should blend colors: {middle:?}"
    );
}

#[test]
fn renderer_fills_a_radial_gradient_rectangle() {
    let animation = Animation::from_json_str(
        r#"{
                "v":"5.7.6",
                "fr":30,
                "ip":0,
                "op":60,
                "w":100,
                "h":100,
                "layers":[
                    {
                        "nm":"Gradient Layer",
                        "ind":1,
                        "ty":4,
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[50,50]},"s":{"a":0,"k":[60,60]},"r":{"a":0,"k":0}},
                                    {"ty":"gf","o":{"a":0,"k":100},"s":{"a":0,"k":[50,50]},"e":{"a":0,"k":[80,50]},"h":{"a":0,"k":0},"a":{"a":0,"k":0},"t":2,"g":{"p":2,"k":{"a":0,"k":[0,1,1,1,1,0,0,0]}}},
                                    {"ty":"tr","a":{"a":0,"k":[0,0]},"p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}
                                ]
                            }
                        ]
                    }
                ]
            }"#,
    )
    .unwrap();

    let frame = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();

    let center = pixel_at(&frame, 50, 50);
    let edge = pixel_at(&frame, 75, 50);

    assert!(
        center[0] > 180 && center[1] > 180 && center[2] > 180,
        "center should be bright: {center:?}"
    );
    assert!(
        edge[0] < 80 && edge[1] < 80 && edge[2] < 80,
        "edge should be dark: {edge:?}"
    );
}

#[test]
fn renderer_fills_a_static_rectangle() {
    let animation = Animation::from_json_str(
        r#"{
                "v":"5.7.6",
                "fr":30,
                "ip":0,
                "op":60,
                "w":100,
                "h":100,
                "layers":[
                    {
                        "nm":"Shape Layer 1",
                        "ind":1,
                        "ty":4,
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[50,50]},"s":{"a":0,"k":[40,20]},"r":{"a":0,"k":0}},
                                    {"ty":"fl","c":{"a":0,"k":[1,0,0,1]},"o":{"a":0,"k":100}},
                                    {"ty":"tr","a":{"a":0,"k":[0,0]},"p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}
                                ]
                            }
                        ]
                    }
                ]
            }"#,
    )
    .unwrap();

    let frame = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();

    let pixel = pixel_at(&frame, 50, 50);
    assert_eq!(pixel, [255, 0, 0, 255]);
    assert_eq!(pixel_at(&frame, 5, 5), [0, 0, 0, 0]);
}

#[test]
fn renderer_draws_shape_siblings_in_reverse_source_order() {
    let animation = Animation::from_json_str(
        r#"{
                "v":"5.7.6",
                "fr":30,
                "ip":0,
                "op":60,
                "w":100,
                "h":100,
                "layers":[
                    {
                        "nm":"Shape Layer 1",
                        "ind":1,
                        "ty":4,
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[50,50]},"s":{"a":0,"k":[20,20]},"r":{"a":0,"k":0}},
                                    {"ty":"fl","c":{"a":0,"k":[0,0,0,1]},"o":{"a":0,"k":100}},
                                    {"ty":"tr","a":{"a":0,"k":[0,0]},"p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}
                                ]
                            },
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[50,50]},"s":{"a":0,"k":[60,60]},"r":{"a":0,"k":0}},
                                    {"ty":"fl","c":{"a":0,"k":[1,0.6,0.8,1]},"o":{"a":0,"k":100}},
                                    {"ty":"tr","a":{"a":0,"k":[0,0]},"p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}
                                ]
                            }
                        ]
                    }
                ]
            }"#,
    )
    .unwrap();

    let frame = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&frame, 50, 50), [0, 0, 0, 255]);
    assert_eq!(pixel_at(&frame, 20, 20), [255, 153, 204, 255]);
}

#[test]
fn renderer_applies_layer_and_group_transforms() {
    let animation = Animation::from_json_str(
        r#"{
                "v":"5.7.6",
                "fr":30,
                "ip":0,
                "op":60,
                "w":100,
                "h":100,
                "layers":[
                    {
                        "nm":"Shape Layer 1",
                        "ind":1,
                        "ty":4,
                        "ks":{"p":{"a":0,"k":[10,0]}},
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[20,20]},"s":{"a":0,"k":[10,10]},"r":{"a":0,"k":0}},
                                    {"ty":"fl","c":{"a":0,"k":[0,1,0,1]},"o":{"a":0,"k":100}},
                                    {"ty":"tr","p":{"a":0,"k":[5,0]},"a":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}
                                ]
                            }
                        ]
                    }
                ]
            }"#,
    )
    .unwrap();

    let frame = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&frame, 35, 20), [0, 255, 0, 255]);
    assert_eq!(pixel_at(&frame, 20, 20), [0, 0, 0, 0]);
}

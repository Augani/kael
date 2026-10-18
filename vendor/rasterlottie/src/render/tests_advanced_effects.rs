#![cfg(test)]

use super::{
    tests::{assert_fixture_supported, pixel_at},
    *,
};
use crate::Animation;

#[test]
fn renderer_draws_single_source_merge_paths_as_the_original_geometry() {
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
                        "nm":"Shape Layer 1",
                        "ind":1,
                        "ty":4,
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[32,32]},"s":{"a":0,"k":[16,16]},"r":{"a":0,"k":0}},
                                    {"ty":"gr","it":[{"ty":"tr","a":{"a":0,"k":[0,0]},"p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}]},
                                    {"ty":"mm","mm":4},
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
    assert_fixture_supported(&animation);

    let frame = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&frame, 32, 32), [255, 0, 0, 255]);
    assert_eq!(pixel_at(&frame, 8, 8), [0, 0, 0, 0]);
}

#[test]
fn renderer_draws_mode1_merge_paths_as_compound_geometry() {
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
                        "nm":"Shape Layer 1",
                        "ind":1,
                        "ty":4,
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"sh","nm":"Outer","ks":{"a":0,"k":{"c":true,"i":[[0,0],[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0],[0,0]],"v":[[8,8],[56,8],[56,56],[8,56]]}}},
                                    {"ty":"sh","nm":"Inner","ks":{"a":0,"k":{"c":true,"i":[[0,0],[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0],[0,0]],"v":[[20,20],[20,44],[44,44],[44,20]]}}},
                                    {"ty":"mm","mm":1},
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
    assert_fixture_supported(&animation);

    let frame = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&frame, 12, 12), [255, 0, 0, 255]);
    assert_eq!(pixel_at(&frame, 32, 32), [0, 0, 0, 0]);
}

#[test]
fn renderer_resolves_supported_path_reference_expressions() {
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
                        "nm":"Head",
                        "ind":1,
                        "ty":4,
                        "hd":true,
                        "shapes":[
                            {
                                "ty":"gr",
                                "nm":"Group 1",
                                "it":[
                                    {
                                        "ty":"sh",
                                        "nm":"Path 1",
                                        "ks":{
                                            "a":0,
                                            "k":{"c":true,"i":[[0,0],[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0],[0,0]],"v":[[20,20],[44,20],[44,44],[20,44]]}
                                        }
                                    }
                                ]
                            }
                        ]
                    },
                    {
                        "nm":"Mask",
                        "ind":2,
                        "ty":4,
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {
                                        "ty":"sh",
                                        "nm":"Path 1",
                                        "ks":{
                                            "x":"var $bm_rt; $bm_rt = thisComp.layer('Head').content('Group 1').content('Path 1').path;"
                                        }
                                    },
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
    assert_fixture_supported(&animation);

    let frame = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&frame, 32, 32), [255, 0, 0, 255]);
    assert_eq!(pixel_at(&frame, 8, 8), [0, 0, 0, 0]);
}

#[test]
fn renderer_applies_supported_fill_effects() {
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
                        "nm":"Shape Layer 1",
                        "ind":1,
                        "ty":4,
                        "ef":[
                            {
                                "mn":"ADBE Fill",
                                "en":1,
                                "ef":[
                                    {"mn":"ADBE Fill-0001","v":{"a":0,"k":0}},
                                    {"mn":"ADBE Fill-0007","v":{"a":0,"k":0}},
                                    {"mn":"ADBE Fill-0002","v":{"a":0,"k":[1,0,0,1]}},
                                    {"mn":"ADBE Fill-0006","v":{"a":0,"k":0}},
                                    {"mn":"ADBE Fill-0003","v":{"a":0,"k":0}},
                                    {"mn":"ADBE Fill-0004","v":{"a":0,"k":0}},
                                    {"mn":"ADBE Fill-0005","v":{"a":0,"k":1}}
                                ]
                            }
                        ],
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[32,32]},"s":{"a":0,"k":[20,20]},"r":{"a":0,"k":0}},
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

    assert_eq!(pixel_at(&frame, 32, 32), [255, 0, 0, 255]);
    assert_eq!(pixel_at(&frame, 8, 8), [0, 0, 0, 0]);
}

#[test]
fn renderer_applies_supported_simple_choker_effects() {
    let animation = Animation::from_json_str(
        r#"{
                "v":"5.7.6",
                "fr":30,
                "ip":0,
                "op":60,
                "w":32,
                "h":32,
                "layers":[
                    {
                        "nm":"Shape Layer 1",
                        "ind":1,
                        "ty":4,
                        "ef":[
                            {
                                "mn":"ADBE Simple Choker",
                                "en":1,
                                "ef":[
                                    {"mn":"ADBE Simple Choker-0001","v":{"a":0,"k":1}},
                                    {"mn":"ADBE Simple Choker-0002","v":{"a":0,"k":1}}
                                ]
                            }
                        ],
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[16,16]},"s":{"a":0,"k":[12,12]},"r":{"a":0,"k":0}},
                                    {"ty":"fl","c":{"a":0,"k":[1,1,1,1]},"o":{"a":0,"k":100}},
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

    assert_eq!(pixel_at(&frame, 16, 16), [255, 255, 255, 255]);
    assert_eq!(pixel_at(&frame, 10, 10)[3], 0);
}

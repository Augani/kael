#![cfg(test)]

use std::{fs, path::PathBuf};

use super::*;
use crate::Animation;

#[test]
fn target_corpus_profile_accepts_an_empty_shape_animation() {
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
                                    {"ty":"rc","nm":"Rect 1"},
                                    {"ty":"fl","nm":"Fill 1"},
                                    {"ty":"tr","nm":"Transform"}
                                ]
                            }
                        ]
                    }
                ]
            }"#,
    )
    .unwrap();

    let report = analyze_animation(&animation);

    assert!(report.is_supported(), "{report}");
}

pub(super) fn load_fixture_animation(name: &str) -> Animation {
    let path = fixture_path(name);
    let json = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read fixture {}: {error}", path.display()));
    Animation::from_json_str(&json)
        .unwrap_or_else(|error| panic!("failed to parse fixture {}: {error}", path.display()))
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[cfg(feature = "text")]
#[test]
fn target_corpus_profile_accepts_glyph_text_layers() {
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

    let report = analyze_animation(&animation);

    assert!(report.is_supported(), "{report}");
}

#[cfg(feature = "text")]
#[test]
fn target_corpus_profile_rejects_text_layers_with_missing_glyphs() {
    let animation = Animation::from_json_str(
        r#"{
                "v":"5.7.6",
                "fr":30,
                "ip":0,
                "op":60,
                "w":128,
                "h":128,
                "fonts":{"list":[{"fName":"TestFont","fFamily":"Test Family","fStyle":"Regular","ascent":75}]},
                "chars":[],
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

    let report = analyze_animation(&animation);

    assert!(report.issues.iter().any(|issue| {
        issue.kind == UnsupportedKind::Text && issue.detail.contains("missing glyph data")
    }));
}

#[cfg(feature = "images")]
#[test]
fn target_corpus_profile_accepts_embedded_image_layers() {
    let animation = Animation::from_json_str(
        r#"{
                "v":"5.7.6",
                "fr":30,
                "ip":0,
                "op":60,
                "w":64,
                "h":64,
                "assets":[{"id":"img","w":1,"h":1,"p":"data:image/png;base64,AAAA","e":1}],
                "layers":[{"nm":"Image","ind":1,"ty":2,"refId":"img"}]
            }"#,
    )
    .unwrap();

    let report = analyze_animation(&animation);

    assert!(report.is_supported(), "{report}");
}

#[cfg(feature = "images")]
#[test]
fn target_corpus_profile_rejects_external_image_assets() {
    let animation = Animation::from_json_str(
        r#"{
                "v":"5.7.6",
                "fr":30,
                "ip":0,
                "op":60,
                "w":64,
                "h":64,
                "assets":[{"id":"img","w":8,"h":8,"u":"images/","p":"cat.png"}],
                "layers":[{"nm":"Image","ind":1,"ty":2,"refId":"img"}]
            }"#,
    )
    .unwrap();

    let report = analyze_animation(&animation);

    assert!(report.issues.iter().any(|issue| {
        issue.kind == UnsupportedKind::ImageAsset && issue.detail.contains("external image assets")
    }));
}

#[cfg(feature = "images")]
#[test]
fn profile_with_external_image_assets_accepts_external_images() {
    let animation = Animation::from_json_str(
        r#"{
                "v":"5.7.6",
                "fr":30,
                "ip":0,
                "op":60,
                "w":64,
                "h":64,
                "assets":[{"id":"img","w":8,"h":8,"u":"images/","p":"cat.png"}],
                "layers":[{"nm":"Image","ind":1,"ty":2,"refId":"img"}]
            }"#,
    )
    .unwrap();

    let report = analyze_animation_with_profile(
        &animation,
        SupportProfile::target_corpus().with_external_image_assets(true),
    );

    assert!(report.is_supported(), "{report}");
}

#[test]
fn target_corpus_profile_rejects_expressions() {
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
                        "ks":{
                            "p":{"a":0,"k":[32,32,0],"x":"time * 10"}
                        }
                    }
                ]
            }"#,
    )
    .unwrap();

    let report = analyze_animation(&animation);

    assert_eq!(report.len(), 1);
    assert_eq!(report.issues[0].kind, UnsupportedKind::Expressions);
    assert!(report.issues[0].path.ends_with(".position"));
}

#[test]
fn target_corpus_profile_accepts_supported_path_reference_expressions() {
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

    let report = analyze_animation(&animation);

    assert!(report.is_supported(), "{report}");
}

#[test]
fn target_corpus_profile_accepts_supported_fill_effects() {
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

    let report = analyze_animation(&animation);

    assert!(report.is_supported(), "{report}");
}

#[test]
fn target_corpus_profile_accepts_supported_simple_choker_effects() {
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
                                "mn":"ADBE Simple Choker",
                                "en":1,
                                "ef":[
                                    {"mn":"ADBE Simple Choker-0001","v":{"a":0,"k":1}},
                                    {"mn":"ADBE Simple Choker-0002","v":{"a":0,"k":2}}
                                ]
                            }
                        ],
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[32,32]},"s":{"a":0,"k":[20,20]},"r":{"a":0,"k":0}},
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

    let report = analyze_animation(&animation);

    assert!(report.is_supported(), "{report}");
}

#[test]
fn target_corpus_profile_accepts_stroke_dashes() {
    let animation = load_fixture_animation("stroke_dash_basic.json");

    let report = analyze_animation(&animation);

    assert!(report.is_supported(), "{report}");
}

#[test]
fn target_corpus_profile_accepts_layer_parenting() {
    let animation = load_fixture_animation("layer_parenting_basic.json");

    let report = analyze_animation(&animation);

    assert!(report.is_supported(), "{report}");
}

#[test]
fn target_corpus_profile_accepts_trim_paths() {
    let animation = load_fixture_animation("trim_path_basic.json");

    let report = analyze_animation(&animation);

    assert!(report.is_supported(), "{report}");
}

#[test]
fn target_corpus_profile_accepts_polystars() {
    let animation = load_fixture_animation("polystar_basic.json");

    let report = analyze_animation(&animation);

    assert!(report.is_supported(), "{report}");
}

#[test]
fn target_corpus_profile_accepts_repeaters() {
    let animation = load_fixture_animation("repeater_basic.json");

    let report = analyze_animation(&animation);

    assert!(report.is_supported(), "{report}");
}

#[test]
fn target_corpus_profile_accepts_precomp_time_remap() {
    let animation = Animation::from_json_str(
        r#"{
                "v":"5.7.6",
                "fr":30,
                "ip":0,
                "op":60,
                "w":64,
                "h":64,
                "assets":[{"id":"pre","layers":[]}],
                "layers":[
                    {
                        "nm":"Precomp Layer",
                        "ind":1,
                        "ty":0,
                        "refId":"pre",
                        "tm":{"a":0,"k":12},
                        "shapes":[]
                    }
                ]
            }"#,
    )
    .unwrap();

    let report = analyze_animation(&animation);

    assert!(report.is_supported(), "{report}");
}

#[test]
fn target_corpus_profile_rejects_non_precomp_time_remap() {
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
                        "tm":{"a":0,"k":12},
                        "shapes":[]
                    }
                ]
            }"#,
    )
    .unwrap();

    let report = analyze_animation(&animation);

    assert!(report.issues.iter().any(|issue| {
        issue.kind == UnsupportedKind::TimeRemap
            && issue.detail.contains("only supported on precomp")
    }));
}

#[test]
fn target_corpus_profile_rejects_non_scalar_time_remap() {
    let animation = Animation::from_json_str(
        r#"{
                "v":"5.7.6",
                "fr":30,
                "ip":0,
                "op":60,
                "w":64,
                "h":64,
                "assets":[{"id":"pre","layers":[]}],
                "layers":[
                    {
                        "nm":"Precomp Layer",
                        "ind":1,
                        "ty":0,
                        "refId":"pre",
                        "tm":{"a":1,"k":[{"t":0,"s":[0,1],"e":[2,3]},{"t":10,"s":[2,3]}]}
                    }
                ]
            }"#,
    )
    .unwrap();

    let report = analyze_animation(&animation);

    assert!(report.issues.iter().any(|issue| {
        issue.kind == UnsupportedKind::TimeRemap && issue.detail.contains("scalar animated value")
    }));
}

#[test]
fn target_corpus_profile_rejects_non_positive_stretch() {
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
                        "sr":0,
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[32,32]},"s":{"a":0,"k":[24,24]},"r":{"a":0,"k":0}},
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

    let report = analyze_animation(&animation);

    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.kind == UnsupportedKind::LayerTiming)
    );
}

#[test]
fn target_corpus_profile_accepts_static_shape_paths() {
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
                                    {"ty":"sh","ks":{"a":0,"k":{"c":true,"i":[[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0]],"v":[[8,56],[32,8],[56,56]]}}},
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

    let report = analyze_animation(&animation);

    assert!(report.is_supported(), "{report}");
}

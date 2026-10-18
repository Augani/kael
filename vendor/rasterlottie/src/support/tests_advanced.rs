#![cfg(test)]

use super::*;
use crate::Animation;

#[test]
fn target_corpus_profile_accepts_basic_animated_shape_paths() {
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
                                    {"ty":"sh","ks":{"a":1,"k":[
                                        {"t":0,"s":[{"c":true,"i":[[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0]],"v":[[8,56],[32,8],[56,56]]}],"e":[{"c":true,"i":[[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0]],"v":[[16,56],[40,8],[64,56]]}],"i":{"x":1,"y":1},"o":{"x":0,"y":0}},
                                        {"t":10,"s":[{"c":true,"i":[[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0]],"v":[[16,56],[40,8],[64,56]]}]}
                                    ]}},
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
fn target_corpus_profile_rejects_incompatible_animated_shape_paths() {
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
                                    {"ty":"sh","ks":{"a":1,"k":[
                                        {"t":0,"s":[{"c":true,"i":[[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0]],"v":[[8,56],[32,8],[56,56]]}],"e":[{"c":true,"i":[[0,0],[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0],[0,0]],"v":[[8,56],[24,8],[40,8],[56,56]]}]},
                                        {"t":10,"s":[{"c":true,"i":[[0,0],[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0],[0,0]],"v":[[8,56],[24,8],[40,8],[56,56]]}]}
                                    ]}},
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
            .any(|issue| issue.detail.contains("shape path uses unsupported"))
    );
}

#[test]
fn target_corpus_profile_accepts_basic_animated_values() {
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
                                    {"ty":"rc","p":{"a":1,"k":[{"t":0,"s":[16,16],"e":[48,16],"i":{"x":[1,1],"y":[1,1]},"o":{"x":[0,0],"y":[0,0]}},{"t":10,"s":[48,16]}]},"s":{"a":0,"k":[12,12]},"r":{"a":0,"k":0}},
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
fn target_corpus_profile_accepts_masks_and_track_mattes() {
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
                        "nm":"Matte Source",
                        "ind":1,
                        "ty":4,
                        "td":1,
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[32,32]},"s":{"a":0,"k":[24,24]},"r":{"a":0,"k":0}},
                                    {"ty":"fl","c":{"a":0,"k":[1,1,1,1]},"o":{"a":0,"k":100}},
                                    {"ty":"tr","a":{"a":0,"k":[0,0]},"p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}
                                ]
                            }
                        ]
                    },
                    {
                        "nm":"Masked Target",
                        "ind":2,
                        "ty":4,
                        "tt":3,
                        "masksProperties":[
                            {
                                "mode":"a",
                                "pt":{"a":0,"k":{"c":true,"i":[[0,0],[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0],[0,0]],"v":[[20,20],[44,20],[44,44],[20,44]]}},
                                "o":{"a":0,"k":100}
                            }
                        ],
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[32,32]},"s":{"a":0,"k":[40,40]},"r":{"a":0,"k":0}},
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
fn target_corpus_profile_accepts_gradient_fills_and_strokes() {
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
                        "nm":"Gradient Layer",
                        "ind":1,
                        "ty":4,
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[32,32]},"s":{"a":0,"k":[28,28]},"r":{"a":0,"k":0}},
                                    {"ty":"gf","o":{"a":0,"k":100},"s":{"a":0,"k":[18,32]},"e":{"a":0,"k":[46,32]},"t":1,"g":{"p":2,"k":{"a":0,"k":[0,1,0,0,1,0,0,1]}}},
                                    {"ty":"gs","o":{"a":0,"k":100},"w":{"a":0,"k":4},"lc":2,"lj":2,"ml":4,"s":{"a":0,"k":[18,32]},"e":{"a":0,"k":[46,32]},"t":1,"g":{"p":2,"k":{"a":0,"k":[0,1,1,1,1,0,0,0]}}},
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
fn target_corpus_profile_rejects_unsupported_gradient_types() {
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
                        "nm":"Gradient Layer",
                        "ind":1,
                        "ty":4,
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[32,32]},"s":{"a":0,"k":[28,28]},"r":{"a":0,"k":0}},
                                    {"ty":"gf","o":{"a":0,"k":100},"s":{"a":0,"k":[18,32]},"e":{"a":0,"k":[46,32]},"t":3,"g":{"p":2,"k":{"a":0,"k":[0,1,0,0,1,0,0,1]}}},
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
            .any(|issue| issue.detail.contains("gradient type"))
    );
}

#[test]
fn target_corpus_profile_rejects_unsupported_mask_modes() {
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
                        "nm":"Masked Layer",
                        "ind":1,
                        "ty":4,
                        "masksProperties":[
                            {
                                "mode":"f",
                                "pt":{"a":0,"k":{"c":true,"i":[[0,0],[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0],[0,0]],"v":[[20,20],[44,20],[44,44],[20,44]]}},
                                "o":{"a":0,"k":100}
                            }
                        ],
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[32,32]},"s":{"a":0,"k":[40,40]},"r":{"a":0,"k":0}},
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

    assert!(report.issues.iter().any(
            |issue| issue.kind == UnsupportedKind::Masks && issue.detail.contains("mask mode")
        ));
}

#[test]
fn target_corpus_profile_rejects_missing_track_matte_sources() {
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
                        "nm":"Target",
                        "ind":2,
                        "ty":4,
                        "tt":1,
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[32,32]},"s":{"a":0,"k":[40,40]},"r":{"a":0,"k":0}},
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

    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.kind == UnsupportedKind::TrackMatte
                && issue.detail.contains("source layer is missing"))
    );
}

#[test]
fn target_corpus_profile_accepts_null_layer_effects() {
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
                        "ef":[{"ty":5,"nm":"Controller"}]
                    }
                ]
            }"#,
    )
    .unwrap();

    let report = analyze_animation(&animation);

    assert!(report.is_supported(), "{report}");
}

#[test]
fn target_corpus_profile_accepts_single_source_merge_paths() {
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

    let report = analyze_animation(&animation);

    assert!(report.is_supported(), "{report}");
}

#[test]
fn target_corpus_profile_accepts_mode1_merge_paths_with_multiple_sources() {
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

    let report = analyze_animation(&animation);

    assert!(report.is_supported(), "{report}");
}

#[test]
fn target_corpus_profile_accepts_trailing_noop_merge_paths_after_a_supported_merge() {
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

    let report = analyze_animation(&animation);

    assert!(report.is_supported(), "{report}");
}

#[test]
fn target_corpus_profile_rejects_merge_paths_with_multiple_sources() {
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
                                    {"ty":"rc","p":{"a":0,"k":[20,32]},"s":{"a":0,"k":[12,12]},"r":{"a":0,"k":0}},
                                    {"ty":"rc","p":{"a":0,"k":[44,32]},"s":{"a":0,"k":[12,12]},"r":{"a":0,"k":0}},
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

    let report = analyze_animation(&animation);

    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.kind == UnsupportedKind::ShapeItem
                && issue.detail.contains("shape item `mm`"))
    );
}

#[test]
fn target_corpus_profile_accepts_split_position() {
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
                            "a":{"a":0,"k":[0,0]},
                            "p":{"s":1,"x":{"a":1,"k":[{"t":0,"s":[16],"e":[48],"i":{"x":1,"y":1},"o":{"x":0,"y":0}},{"t":10,"s":[48]}]},"y":{"a":0,"k":32}},
                            "s":{"a":0,"k":[100,100]},
                            "r":{"a":0,"k":0},
                            "o":{"a":0,"k":100}
                        },
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[12,12]},"r":{"a":0,"k":0}},
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
fn target_corpus_profile_rejects_malformed_split_position() {
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
                            "a":{"a":0,"k":[0,0]},
                            "p":{"s":1,"x":{"a":0,"k":16}},
                            "s":{"a":0,"k":[100,100]},
                            "r":{"a":0,"k":0},
                            "o":{"a":0,"k":100}
                        },
                        "shapes":[]
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
            .any(|issue| issue.detail.contains("split position data is malformed"))
    );
}

#[test]
fn target_corpus_profile_accepts_spatial_keyframes() {
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
                                    {"ty":"rc","p":{"a":1,"k":[{"t":0,"s":[16,48],"e":[48,48],"to":[0,-24],"ti":[0,-24],"i":{"x":[1,1],"y":[1,1]},"o":{"x":[0,0],"y":[0,0]}},{"t":10,"s":[48,48]}]},"s":{"a":0,"k":[12,12]},"r":{"a":0,"k":0}},
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
fn target_corpus_profile_rejects_malformed_spatial_keyframes() {
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
                                    {"ty":"rc","p":{"a":1,"k":[{"t":0,"s":[16,48],"e":[48,48],"to":[0,-24],"i":{"x":[1,1],"y":[1,1]},"o":{"x":[0,0],"y":[0,0]}},{"t":10,"s":[48,48]}]},"s":{"a":0,"k":[12,12]},"r":{"a":0,"k":0}},
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
            .any(|issue| issue.kind == UnsupportedKind::AnimatedValue)
    );
}

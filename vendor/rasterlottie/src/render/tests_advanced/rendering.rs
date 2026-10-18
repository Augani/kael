use super::super::{tests::pixel_at, *};
use crate::{Animation, analyze_animation};

#[test]
fn renderer_respects_rounded_rectangle_corners() {
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
                                    {"ty":"rc","p":{"a":0,"k":[50,50]},"s":{"a":0,"k":[40,40]},"r":{"a":0,"k":10}},
                                    {"ty":"fl","c":{"a":0,"k":[1,0.5,0,1]},"o":{"a":0,"k":100}},
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

    assert_eq!(pixel_at(&frame, 31, 31), [0, 0, 0, 0]);
    assert_eq!(pixel_at(&frame, 40, 40), [255, 128, 0, 255]);
}

#[test]
fn renderer_fills_a_static_ellipse() {
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
                                    {"ty":"el","p":{"a":0,"k":[50,50]},"s":{"a":0,"k":[30,20]}},
                                    {"ty":"fl","c":{"a":0,"k":[0,0,1,1]},"o":{"a":0,"k":100}},
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

    assert_eq!(pixel_at(&frame, 50, 50), [0, 0, 255, 255]);
    assert_eq!(pixel_at(&frame, 20, 20), [0, 0, 0, 0]);
}

#[test]
fn renderer_strokes_a_static_rectangle() {
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
                                    {"ty":"rc","p":{"a":0,"k":[50,50]},"s":{"a":0,"k":[40,40]},"r":{"a":0,"k":0}},
                                    {"ty":"st","c":{"a":0,"k":[1,1,1,1]},"o":{"a":0,"k":100},"w":{"a":0,"k":8},"lc":2,"lj":2,"ml":4},
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

    assert_eq!(pixel_at(&frame, 30, 50), [255, 255, 255, 255]);
    assert_eq!(pixel_at(&frame, 50, 50), [0, 0, 0, 0]);
}

#[test]
fn renderer_fills_a_static_shape_path() {
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
                                    {"ty":"sh","ks":{"a":0,"k":{"c":true,"i":[[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0]],"v":[[20,80],[50,20],[80,80]]}}},
                                    {"ty":"fl","c":{"a":0,"k":[0.5,0,1,1]},"o":{"a":0,"k":100}},
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

    assert_eq!(pixel_at(&frame, 50, 60), [128, 0, 255, 255]);
    assert_eq!(pixel_at(&frame, 10, 10), [0, 0, 0, 0]);
}

#[test]
fn renderer_strokes_an_open_shape_path() {
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
                                    {"ty":"sh","ks":{"a":0,"k":{"c":false,"i":[[0,0],[0,0]],"o":[[0,0],[0,0]],"v":[[20,20],[80,80]]}}},
                                    {"ty":"st","c":{"a":0,"k":[1,1,0,1]},"o":{"a":0,"k":100},"w":{"a":0,"k":6},"lc":2,"lj":2,"ml":4},
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

    assert_eq!(pixel_at(&frame, 50, 50), [255, 255, 0, 255]);
    assert_eq!(pixel_at(&frame, 50, 20), [0, 0, 0, 0]);
}

#[test]
fn renderer_interpolates_animated_rectangle_position() {
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
                                    {"ty":"rc","p":{"a":1,"k":[{"t":0,"s":[20,50],"e":[80,50],"i":{"x":[1,1],"y":[1,1]},"o":{"x":[0,0],"y":[0,0]}},{"t":10,"s":[80,50]}]},"s":{"a":0,"k":[20,20]},"r":{"a":0,"k":0}},
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

    let early = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();
    let middle = Renderer::default()
        .render_frame(&animation, 5.0, RenderConfig::default())
        .unwrap();
    let late = Renderer::default()
        .render_frame(&animation, 10.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&early, 20, 50), [255, 0, 0, 255]);
    assert_eq!(pixel_at(&middle, 50, 50), [255, 0, 0, 255]);
    assert_eq!(pixel_at(&late, 80, 50), [255, 0, 0, 255]);
}

#[test]
fn renderer_interpolates_split_position() {
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
                        "ks":{
                            "a":{"a":0,"k":[0,0]},
                            "p":{"s":1,"x":{"a":1,"k":[{"t":0,"s":[20],"e":[80],"i":{"x":1,"y":1},"o":{"x":0,"y":0}},{"t":10,"s":[80]}]},"y":{"a":0,"k":50}},
                            "s":{"a":0,"k":[100,100]},
                            "r":{"a":0,"k":0},
                            "o":{"a":0,"k":100}
                        },
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[20,20]},"r":{"a":0,"k":0}},
                                    {"ty":"fl","c":{"a":0,"k":[1,0.5,0,1]},"o":{"a":0,"k":100}},
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

    let early = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();
    let middle = Renderer::default()
        .render_frame(&animation, 5.0, RenderConfig::default())
        .unwrap();
    let late = Renderer::default()
        .render_frame(&animation, 10.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&early, 20, 50), [255, 128, 0, 255]);
    assert_eq!(pixel_at(&middle, 50, 50), [255, 128, 0, 255]);
    assert_eq!(pixel_at(&late, 80, 50), [255, 128, 0, 255]);
}

#[test]
fn renderer_interpolates_spatial_rectangle_position() {
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
                                    {"ty":"rc","p":{"a":1,"k":[{"t":0,"s":[20,80],"e":[80,80],"to":[0,-80],"ti":[0,-80],"i":{"x":1,"y":1},"o":{"x":0,"y":0}},{"t":10,"s":[80,80]}]},"s":{"a":0,"k":[16,16]},"r":{"a":0,"k":0}},
                                    {"ty":"fl","c":{"a":0,"k":[0,0,1,1]},"o":{"a":0,"k":100}},
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

    let early = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();
    let middle = Renderer::default()
        .render_frame(&animation, 5.0, RenderConfig::default())
        .unwrap();
    let late = Renderer::default()
        .render_frame(&animation, 10.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&early, 20, 80), [0, 0, 255, 255]);
    assert_eq!(pixel_at(&middle, 50, 20), [0, 0, 255, 255]);
    assert_eq!(pixel_at(&late, 80, 80), [0, 0, 255, 255]);
}

#[test]
fn renderer_interpolates_animated_shape_paths() {
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
                                    {"ty":"sh","ks":{"a":1,"k":[
                                        {"t":0,"s":[{"c":true,"i":[[0,0],[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0],[0,0]],"v":[[10,30],[30,30],[30,70],[10,70]]}],"e":[{"c":true,"i":[[0,0],[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0],[0,0]],"v":[[70,30],[90,30],[90,70],[70,70]]}],"i":{"x":1,"y":1},"o":{"x":0,"y":0}},
                                        {"t":10,"s":[{"c":true,"i":[[0,0],[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0],[0,0]],"v":[[70,30],[90,30],[90,70],[70,70]]}]}
                                    ]}},
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

    let early = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();
    let middle = Renderer::default()
        .render_frame(&animation, 5.0, RenderConfig::default())
        .unwrap();
    let late = Renderer::default()
        .render_frame(&animation, 10.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&early, 20, 50), [0, 255, 0, 255]);
    assert_eq!(pixel_at(&middle, 50, 50), [0, 255, 0, 255]);
    assert_eq!(pixel_at(&late, 80, 50), [0, 255, 0, 255]);
}

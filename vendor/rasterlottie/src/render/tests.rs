#![cfg(test)]

use std::{fs, path::PathBuf};

#[cfg(feature = "images")]
use base64::{Engine, engine::general_purpose::STANDARD};
#[cfg(feature = "images")]
use image::{ColorType, ImageEncoder, Rgba, RgbaImage, codecs::png::PngEncoder};

use super::*;
use crate::{Animation, analyze_animation};

#[test]
fn renderer_can_produce_a_blank_frame_for_an_empty_animation() {
    let animation =
        Animation::from_json_str(r#"{"v":"5.7.6","fr":30,"ip":0,"op":10,"w":2,"h":1,"layers":[]}"#)
            .unwrap();

    let frame = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();

    assert_eq!(frame.width, 2);
    assert_eq!(frame.height, 1);
    assert_eq!(frame.pixels, vec![0, 0, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn prepared_animation_reuses_the_same_render_result() {
    let animation = Animation::from_json_str(
        r#"{
            "v":"5.7.6",
            "fr":30,
            "ip":0,
            "op":10,
            "w":16,
            "h":16,
            "layers":[
                {
                    "nm":"Shape Layer 1",
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

    let renderer = Renderer::default();
    let direct = renderer
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();
    let prepared = renderer.prepare(&animation).unwrap();
    let reused = prepared.render_frame(0.0, RenderConfig::default()).unwrap();
    let mut scratch = prepared.new_scratch_pixmap().unwrap();
    prepared
        .render_frame_into_pixmap(0.0, RenderConfig::default(), &mut scratch)
        .unwrap();

    assert_eq!(prepared.animation().width, animation.width);
    assert_eq!(prepared.animation().height, animation.height);
    assert_eq!(direct, reused);
    assert_eq!(direct.pixels, scratch.data());
}

#[test]
fn renderer_can_render_at_a_scaled_output_size() {
    let animation = Animation::from_json_str(
        r#"{
            "v":"5.7.6",
            "fr":30,
            "ip":0,
            "op":10,
            "w":16,
            "h":16,
            "layers":[
                {
                    "nm":"Shape Layer 1",
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

    let frame = Renderer::default()
        .render_frame(
            &animation,
            0.0,
            RenderConfig {
                scale: 2.0,
                ..RenderConfig::default()
            },
        )
        .unwrap();

    assert_eq!(frame.width, 32);
    assert_eq!(frame.height, 32);
    assert_eq!(pixel_at(&frame, 16, 16), [255, 0, 0, 255]);
}

#[test]
fn renderer_respects_animation_frame_window() {
    let animation = Animation::from_json_str(
            r#"{
                "v":"5.7.6",
                "fr":30,
                "ip":10,
                "op":20,
                "w":16,
                "h":16,
                "layers":[
                    {
                        "nm":"Shape Layer 1",
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

    let before = Renderer::default()
        .render_frame(&animation, 5.0, RenderConfig::default())
        .unwrap();
    let active = Renderer::default()
        .render_frame(&animation, 10.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&before, 8, 8), [0, 0, 0, 0]);
    assert_eq!(pixel_at(&active, 8, 8), [255, 0, 0, 255]);
}

#[test]
fn renderer_respects_layer_frame_window() {
    let animation = Animation::from_json_str(
            r#"{
                "v":"5.7.6",
                "fr":30,
                "ip":0,
                "op":60,
                "w":16,
                "h":16,
                "layers":[
                    {
                        "nm":"Shape Layer 1",
                        "ind":1,
                        "ty":4,
                        "ip":10,
                        "op":20,
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[8,8]},"s":{"a":0,"k":[8,8]},"r":{"a":0,"k":0}},
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

    let before = Renderer::default()
        .render_frame(&animation, 9.0, RenderConfig::default())
        .unwrap();
    let active = Renderer::default()
        .render_frame(&animation, 10.0, RenderConfig::default())
        .unwrap();
    let after = Renderer::default()
        .render_frame(&animation, 20.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&before, 8, 8), [0, 0, 0, 0]);
    assert_eq!(pixel_at(&active, 8, 8), [0, 255, 0, 255]);
    assert_eq!(pixel_at(&after, 8, 8), [0, 0, 0, 0]);
}

#[test]
fn renderer_maps_precomp_time_with_start_time_and_stretch() {
    let animation = Animation::from_json_str(
            r#"{
                "v":"5.7.6",
                "fr":30,
                "ip":0,
                "op":60,
                "w":32,
                "h":32,
                "assets":[
                    {
                        "id":"pre",
                        "layers":[
                            {
                                "nm":"Child Shape",
                                "ind":1,
                                "ty":4,
                                "ip":0,
                                "op":10,
                                "shapes":[
                                    {
                                        "ty":"gr",
                                        "it":[
                                            {"ty":"rc","p":{"a":0,"k":[16,16]},"s":{"a":0,"k":[12,12]},"r":{"a":0,"k":0}},
                                            {"ty":"fl","c":{"a":0,"k":[0,0,1,1]},"o":{"a":0,"k":100}},
                                            {"ty":"tr","a":{"a":0,"k":[0,0]},"p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}
                                        ]
                                    }
                                ]
                            }
                        ]
                    }
                ],
                "layers":[
                    {
                        "nm":"Precomp Layer",
                        "ind":1,
                        "ty":0,
                        "refId":"pre",
                        "st":5,
                        "sr":2,
                        "ip":0,
                        "op":40
                    }
                ]
            }"#,
        )
        .unwrap();

    let before = Renderer::default()
        .render_frame(&animation, 9.0, RenderConfig::default())
        .unwrap();
    let start = Renderer::default()
        .render_frame(&animation, 10.0, RenderConfig::default())
        .unwrap();
    let late = Renderer::default()
        .render_frame(&animation, 29.0, RenderConfig::default())
        .unwrap();
    let end = Renderer::default()
        .render_frame(&animation, 30.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&before, 16, 16), [0, 0, 0, 0]);
    assert_eq!(pixel_at(&start, 16, 16), [0, 0, 255, 255]);
    assert_eq!(pixel_at(&late, 16, 16), [0, 0, 255, 255]);
    assert_eq!(pixel_at(&end, 16, 16), [0, 0, 0, 0]);
}

#[test]
fn renderer_applies_static_precomp_time_remap() {
    let animation = Animation::from_json_str(
        r#"{
                "v":"5.7.6",
                "fr":10,
                "ip":0,
                "op":20,
                "w":32,
                "h":32,
                "assets":[
                    {
                        "id":"pre",
                        "layers":[
                            {
                                "nm":"Green",
                                "ind":1,
                                "ty":4,
                                "ip":0,
                                "op":1,
                                "shapes":[
                                    {
                                        "ty":"gr",
                                        "it":[
                                            {"ty":"rc","p":{"a":0,"k":[16,16]},"s":{"a":0,"k":[12,12]},"r":{"a":0,"k":0}},
                                            {"ty":"fl","c":{"a":0,"k":[0,1,0,1]},"o":{"a":0,"k":100}},
                                            {"ty":"tr","a":{"a":0,"k":[0,0]},"p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}
                                        ]
                                    }
                                ]
                            },
                            {
                                "nm":"Red",
                                "ind":2,
                                "ty":4,
                                "ip":10,
                                "op":11,
                                "shapes":[
                                    {
                                        "ty":"gr",
                                        "it":[
                                            {"ty":"rc","p":{"a":0,"k":[16,16]},"s":{"a":0,"k":[12,12]},"r":{"a":0,"k":0}},
                                            {"ty":"fl","c":{"a":0,"k":[1,0,0,1]},"o":{"a":0,"k":100}},
                                            {"ty":"tr","a":{"a":0,"k":[0,0]},"p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}
                                        ]
                                    }
                                ]
                            }
                        ]
                    }
                ],
                "layers":[
                    {
                        "nm":"Precomp Layer",
                        "ind":1,
                        "ty":0,
                        "refId":"pre",
                        "tm":{"a":0,"k":1}
                    }
                ]
            }"#,
    )
    .unwrap();

    assert!(analyze_animation(&animation).is_supported());

    let frame = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&frame, 16, 16), [255, 0, 0, 255]);
}

#[test]
fn renderer_interpolates_precomp_time_remap() {
    let animation = Animation::from_json_str(
        r#"{
                "v":"5.7.6",
                "fr":10,
                "ip":0,
                "op":20,
                "w":32,
                "h":32,
                "assets":[
                    {
                        "id":"pre",
                        "layers":[
                            {
                                "nm":"Green",
                                "ind":1,
                                "ty":4,
                                "ip":0,
                                "op":1,
                                "shapes":[
                                    {
                                        "ty":"gr",
                                        "it":[
                                            {"ty":"rc","p":{"a":0,"k":[16,16]},"s":{"a":0,"k":[12,12]},"r":{"a":0,"k":0}},
                                            {"ty":"fl","c":{"a":0,"k":[0,1,0,1]},"o":{"a":0,"k":100}},
                                            {"ty":"tr","a":{"a":0,"k":[0,0]},"p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}
                                        ]
                                    }
                                ]
                            },
                            {
                                "nm":"Red",
                                "ind":2,
                                "ty":4,
                                "ip":10,
                                "op":11,
                                "shapes":[
                                    {
                                        "ty":"gr",
                                        "it":[
                                            {"ty":"rc","p":{"a":0,"k":[16,16]},"s":{"a":0,"k":[12,12]},"r":{"a":0,"k":0}},
                                            {"ty":"fl","c":{"a":0,"k":[1,0,0,1]},"o":{"a":0,"k":100}},
                                            {"ty":"tr","a":{"a":0,"k":[0,0]},"p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}
                                        ]
                                    }
                                ]
                            }
                        ]
                    }
                ],
                "layers":[
                    {
                        "nm":"Precomp Layer",
                        "ind":1,
                        "ty":0,
                        "refId":"pre",
                        "tm":{
                            "a":1,
                            "k":[
                                {"t":0,"s":[0],"e":[1],"i":{"x":[1],"y":[1]},"o":{"x":[0],"y":[0]}},
                                {"t":10,"s":[1]}
                            ]
                        }
                    }
                ]
            }"#,
    )
    .unwrap();

    assert!(analyze_animation(&animation).is_supported());

    let start = Renderer::default()
        .render_frame(&animation, 0.0, RenderConfig::default())
        .unwrap();
    let end = Renderer::default()
        .render_frame(&animation, 10.0, RenderConfig::default())
        .unwrap();

    assert_eq!(pixel_at(&start, 16, 16), [0, 255, 0, 255]);
    assert_eq!(pixel_at(&end, 16, 16), [255, 0, 0, 255]);
}

pub(super) fn pixel_at(frame: &RasterFrame, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * frame.width + x) * 4) as usize;
    [
        frame.pixels[idx],
        frame.pixels[idx + 1],
        frame.pixels[idx + 2],
        frame.pixels[idx + 3],
    ]
}

pub(super) fn color_sum(pixel: [u8; 4]) -> u16 {
    u16::from(pixel[0]) + u16::from(pixel[1]) + u16::from(pixel[2])
}

pub(super) fn assert_fixture_supported(animation: &Animation) {
    let report = analyze_animation(animation);
    assert!(report.is_supported(), "{report}");
}

pub(super) fn load_fixture_animation(name: &str) -> Animation {
    let path = fixture_path(name);
    let json = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read fixture {}: {error}", path.display()));
    Animation::from_json_str(&json)
        .unwrap_or_else(|error| panic!("failed to parse fixture {}: {error}", path.display()))
}

#[cfg(feature = "images")]
pub(super) fn solid_png_data_url(color: [u8; 4]) -> String {
    format!(
        "data:image/png;base64,{}",
        STANDARD.encode(solid_png_bytes(color))
    )
}

#[cfg(feature = "images")]
pub(super) fn solid_png_bytes(color: [u8; 4]) -> Vec<u8> {
    let image = RgbaImage::from_pixel(1, 1, Rgba(color));
    let mut bytes = Vec::new();
    PngEncoder::new(&mut bytes)
        .write_image(image.as_raw(), 1, 1, ColorType::Rgba8.into())
        .unwrap();
    bytes
}

pub(super) fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

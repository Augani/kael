use std::io::Cursor;

use super::super::{PreparedAnimation, RasterFrame, Renderer, *};
use crate::Animation;

#[derive(Debug, PartialEq, Eq)]
struct DecodedGifFrame {
    delay: u16,
    dispose: ::gif::DisposalMethod,
    raster: RasterFrame,
}

#[derive(Debug, PartialEq, Eq)]
struct RawGifFrame {
    delay: u16,
    dispose: ::gif::DisposalMethod,
    left: u16,
    top: u16,
    raster: RasterFrame,
}

fn decode_gif_raw_frames(bytes: &[u8]) -> (u16, u16, Vec<RawGifFrame>) {
    let mut decoder = ::gif::DecodeOptions::new();
    decoder.set_color_output(::gif::ColorOutput::RGBA);
    let mut decoder = decoder.read_info(Cursor::new(bytes)).unwrap();
    let canvas_width = decoder.width();
    let canvas_height = decoder.height();
    let mut frames = Vec::new();

    while let Some(frame) = decoder.read_next_frame().unwrap() {
        frames.push(RawGifFrame {
            delay: frame.delay,
            dispose: frame.dispose,
            left: frame.left,
            top: frame.top,
            raster: RasterFrame {
                width: u32::from(frame.width),
                height: u32::from(frame.height),
                pixels: frame.buffer.to_vec(),
            },
        });
    }

    (canvas_width, canvas_height, frames)
}

fn decode_gif_rgba_frames(bytes: &[u8]) -> Vec<DecodedGifFrame> {
    let (canvas_width, canvas_height, raw_frames) = decode_gif_raw_frames(bytes);
    let mut canvas = vec![0; usize::from(canvas_width) * usize::from(canvas_height) * 4];
    let mut previous_canvas = None;
    let mut pending_disposal = None;
    let mut frames = Vec::new();

    for frame in raw_frames {
        if let Some((dispose, left, top, width, height)) = pending_disposal.take() {
            match dispose {
                ::gif::DisposalMethod::Background => {
                    clear_gif_rect(&mut canvas, canvas_width, left, top, width, height);
                }
                ::gif::DisposalMethod::Previous => {
                    if let Some(previous) = previous_canvas.take() {
                        canvas = previous;
                    }
                }
                _ => {}
            }
        }

        previous_canvas =
            matches!(frame.dispose, ::gif::DisposalMethod::Previous).then(|| canvas.clone());
        composite_gif_rgba_frame(&mut canvas, canvas_width, &frame);
        frames.push(DecodedGifFrame {
            delay: frame.delay,
            dispose: frame.dispose,
            raster: RasterFrame::new(
                u32::from(canvas_width),
                u32::from(canvas_height),
                canvas.clone(),
            ),
        });
        pending_disposal = Some((
            frame.dispose,
            frame.left,
            frame.top,
            frame.raster.width as u16,
            frame.raster.height as u16,
        ));
    }

    frames
}

fn assert_gif_visual_frames_eq(left: &[DecodedGifFrame], right: &[DecodedGifFrame]) {
    assert_eq!(left.len(), right.len());
    for (index, (left, right)) in left.iter().zip(right).enumerate() {
        assert_eq!(left.delay, right.delay, "frame {index} delay mismatch");
        assert_eq!(left.raster, right.raster, "frame {index} raster mismatch");
    }
}

fn composite_gif_rgba_frame(canvas: &mut [u8], canvas_width: u16, frame: &RawGifFrame) {
    let canvas_width = usize::from(canvas_width);
    let frame_width = frame.raster.width as usize;
    let frame_height = frame.raster.height as usize;
    let left = usize::from(frame.left);
    let top = usize::from(frame.top);

    for y in 0..frame_height {
        for x in 0..frame_width {
            let frame_offset = ((y * frame_width) + x) * 4;
            if frame.raster.pixels[frame_offset + 3] == 0 {
                continue;
            }

            let canvas_offset = (((top + y) * canvas_width) + left + x) * 4;
            canvas[canvas_offset..canvas_offset + 4]
                .copy_from_slice(&frame.raster.pixels[frame_offset..frame_offset + 4]);
        }
    }
}

fn clear_gif_rect(
    canvas: &mut [u8],
    canvas_width: u16,
    left: u16,
    top: u16,
    width: u16,
    height: u16,
) {
    let canvas_width = usize::from(canvas_width);
    let left = usize::from(left);
    let top = usize::from(top);
    let width = usize::from(width);
    let height = usize::from(height);

    for y in top..top + height {
        let row_start = ((y * canvas_width) + left) * 4;
        let row_end = row_start + width * 4;
        canvas[row_start..row_end].fill(0);
    }
}

fn moving_rect_gif_animation() -> Animation {
    Animation::from_json_str(
        r#"{
                "v":"5.7.6",
                "fr":60,
                "ip":0,
                "op":60,
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
                                    {"ty":"rc","p":{"a":1,"k":[{"t":0,"s":[4,8],"e":[12,8],"i":{"x":[1,1],"y":[1,1]},"o":{"x":[0,0],"y":[0,0]}},{"t":59,"s":[12,8]}]},"s":{"a":0,"k":[4,4]},"r":{"a":0,"k":0}},
                                    {"ty":"fl","c":{"a":0,"k":[1,0,0,1]},"o":{"a":0,"k":100}},
                                    {"ty":"tr","a":{"a":0,"k":[0,0]},"p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}
                                ]
                            }
                        ]
                    }
                ]
            }"#,
    )
    .unwrap()
}

fn moving_rect_over_opaque_bg_animation() -> Animation {
    Animation::from_json_str(
        r#"{
                "v":"5.7.6",
                "fr":10,
                "ip":0,
                "op":10,
                "w":32,
                "h":32,
                "layers":[
                    {
                        "nm":"Moving Rect",
                        "ind":2,
                        "ty":4,
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":1,"k":[{"t":0,"s":[8,16],"e":[24,16],"i":{"x":[1,1],"y":[1,1]},"o":{"x":[0,0],"y":[0,0]}},{"t":9,"s":[24,16]}]},"s":{"a":0,"k":[8,8]},"r":{"a":0,"k":0}},
                                    {"ty":"fl","c":{"a":0,"k":[1,0,0,1]},"o":{"a":0,"k":100}},
                                    {"ty":"tr","a":{"a":0,"k":[0,0]},"p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}
                                ]
                            }
                        ]
                    },
                    {
                        "nm":"Background",
                        "ind":1,
                        "ty":4,
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":0,"k":[16,16]},"s":{"a":0,"k":[32,32]},"r":{"a":0,"k":0}},
                                    {"ty":"fl","c":{"a":0,"k":[1,1,1,1]},"o":{"a":0,"k":100}},
                                    {"ty":"tr","a":{"a":0,"k":[0,0]},"p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}
                                ]
                            }
                        ]
                    }
                ]
            }"#,
    )
    .unwrap()
}

fn build_test_gif_frame_jobs(animation: &Animation, config: GifRenderConfig) -> Vec<(f32, u16)> {
    let source_fps = animation.frame_rate.max(1.0);
    let requested_fps = source_fps.min(config.max_fps.max(1.0));
    let start_frame = animation.in_point.floor();
    let end_frame = animation.out_point.ceil().max(start_frame + 1.0);
    let output_duration_seconds =
        ((end_frame - start_frame) / source_fps).min(config.max_duration_seconds.max(0.1));
    let max_output_frames = (requested_fps * output_duration_seconds).floor().max(1.0) as usize;
    let source_frame_step = source_fps / requested_fps;

    let mut frames = Vec::with_capacity(max_output_frames);
    let mut previous_deadline_centiseconds = 0u32;
    for rendered in 0..max_output_frames {
        let source_frame = (rendered as f32).mul_add(source_frame_step, start_frame);
        if source_frame >= end_frame {
            break;
        }

        let next_deadline_centiseconds =
            ((((rendered + 1) as f64) * 100.0) / f64::from(requested_fps)).round() as u32;
        let next_deadline_centiseconds =
            next_deadline_centiseconds.max(previous_deadline_centiseconds + 1);
        let delay = u16::try_from(next_deadline_centiseconds - previous_deadline_centiseconds)
            .unwrap_or(u16::MAX);
        previous_deadline_centiseconds = next_deadline_centiseconds;
        frames.push((source_frame, delay));
    }

    frames
}

fn render_full_frame_gif_reference(
    prepared: &PreparedAnimation,
    config: GifRenderConfig,
) -> Vec<DecodedGifFrame> {
    let frame_jobs = build_test_gif_frame_jobs(prepared.animation(), config);
    let mut scratch = prepared
        .new_scratch_pixmap_for_config(config.render)
        .unwrap();
    let mut bytes = Vec::new();
    let mut encoder = ::gif::Encoder::new(
        &mut bytes,
        scratch.width() as u16,
        scratch.height() as u16,
        &[],
    )
    .unwrap();
    encoder.set_repeat(::gif::Repeat::Infinite).unwrap();

    for (source_frame, delay) in frame_jobs {
        prepared
            .render_frame_into_pixmap(source_frame, config.render, &mut scratch)
            .unwrap();
        let mut frame = super::super::gif_encode::encode_rgba_frame(
            scratch.width() as u16,
            scratch.height() as u16,
            scratch.data_mut(),
            config.color_quantizer_speed,
        );
        frame.delay = delay;
        frame.dispose = ::gif::DisposalMethod::Background;
        encoder.write_frame(&frame).unwrap();
    }
    drop(encoder);

    decode_gif_rgba_frames(&bytes)
}

#[test]
fn renderer_encodes_a_gif_for_animated_content() {
    let animation = Animation::from_json_str(
        r#"{
                "v":"5.7.6",
                "fr":10,
                "ip":0,
                "op":10,
                "w":32,
                "h":32,
                "layers":[
                    {
                        "nm":"Shape Layer 1",
                        "ind":1,
                        "ty":4,
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {"ty":"rc","p":{"a":1,"k":[{"t":0,"s":[8,16],"e":[24,16],"i":{"x":[1,1],"y":[1,1]},"o":{"x":[0,0],"y":[0,0]}},{"t":9,"s":[24,16]}]},"s":{"a":0,"k":[8,8]},"r":{"a":0,"k":0}},
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

    let bytes = Renderer::default()
        .render_gif(&animation, GifRenderConfig::default())
        .unwrap();

    assert!(bytes.starts_with(b"GIF89a") || bytes.starts_with(b"GIF87a"));
    let frames = decode_gif_rgba_frames(&bytes);
    let (_canvas_width, _canvas_height, raw_frames) = decode_gif_raw_frames(&bytes);

    assert_eq!(frames.len(), 10);
    assert!(frames.iter().all(|frame| frame.delay == 10));
    assert!(!raw_frames.is_empty());

    let first = &frames[0].raster;
    let last = &frames[frames.len() - 1].raster;
    assert_eq!(
        super::super::tests::pixel_at(first, 8, 16),
        [255, 0, 0, 255]
    );
    assert_eq!(super::super::tests::pixel_at(first, 24, 16), [0, 0, 0, 0]);
    assert_eq!(
        super::super::tests::pixel_at(last, 24, 16),
        [255, 0, 0, 255]
    );
    assert_eq!(super::super::tests::pixel_at(last, 8, 16), [0, 0, 0, 0]);
    assert_ne!(first, last);
}

#[test]
fn renderer_uses_background_disposal_for_gif_frames() {
    let animation = moving_rect_gif_animation();

    let bytes = Renderer::default()
        .render_gif(&animation, GifRenderConfig::default())
        .unwrap();

    let (_canvas_width, _canvas_height, frames) = decode_gif_raw_frames(&bytes);
    let disposals: Vec<_> = frames.iter().map(|frame| frame.dispose).collect();

    assert!(!disposals.is_empty());
    assert!(disposals.contains(&::gif::DisposalMethod::Background));
}

#[test]
fn renderer_quantizes_gif_frame_rate_to_centiseconds() {
    let animation = moving_rect_gif_animation();

    let bytes = Renderer::default()
        .render_gif(
            &animation,
            GifRenderConfig {
                max_fps: 60.0,
                max_duration_seconds: 1.0,
                ..GifRenderConfig::default()
            },
        )
        .unwrap();

    let frames = decode_gif_rgba_frames(&bytes);

    assert_eq!(frames.len(), 60);
    assert!(frames.iter().all(|frame| matches!(frame.delay, 1 | 2)));
    assert_eq!(
        frames
            .iter()
            .map(|frame| u32::from(frame.delay))
            .sum::<u32>(),
        100
    );
    assert_eq!(
        super::super::tests::pixel_at(&frames[0].raster, 4, 8),
        [255, 0, 0, 255]
    );
    assert_eq!(
        super::super::tests::pixel_at(&frames[frames.len() - 1].raster, 12, 8),
        [255, 0, 0, 255]
    );
}

#[test]
fn renderer_distributes_gif_delays_for_non_integer_centisecond_frame_rate() {
    let animation = moving_rect_gif_animation();

    let bytes = Renderer::default()
        .render_gif(
            &animation,
            GifRenderConfig {
                max_fps: 15.0,
                max_duration_seconds: 1.0,
                ..GifRenderConfig::default()
            },
        )
        .unwrap();

    let frames = decode_gif_rgba_frames(&bytes);
    let delays: Vec<u16> = frames.iter().map(|frame| frame.delay).collect();

    assert_eq!(frames.len(), 15);
    assert!(delays.iter().all(|delay| matches!(delay, 6 | 7)));
    assert!(delays.contains(&6));
    assert!(delays.contains(&7));
    assert_eq!(
        delays.iter().map(|delay| u32::from(*delay)).sum::<u32>(),
        100
    );
}

#[test]
fn prepared_animation_parallel_gif_matches_sequential_output() {
    let animation = moving_rect_gif_animation();
    let prepared = Renderer::default().prepare(&animation).unwrap();
    let config = GifRenderConfig {
        max_fps: 15.0,
        max_duration_seconds: 1.0,
        ..GifRenderConfig::default()
    };

    let sequential = prepared
        .render_gif_with_parallelism_for_test(config, 1)
        .unwrap();
    let parallel = prepared
        .render_gif_with_parallelism_for_test(config, 2)
        .unwrap();

    assert_eq!(parallel, sequential);
}

#[test]
fn cropped_gif_matches_full_frame_visual_output() {
    let animation = moving_rect_gif_animation();
    let prepared = Renderer::default().prepare(&animation).unwrap();
    let config = GifRenderConfig {
        max_fps: 15.0,
        max_duration_seconds: 1.0,
        ..GifRenderConfig::default()
    };

    let cropped = prepared.render_gif(config).unwrap();
    let cropped_frames = decode_gif_rgba_frames(&cropped);
    let full_frame_frames = render_full_frame_gif_reference(&prepared, config);

    assert_gif_visual_frames_eq(&cropped_frames, &full_frame_frames);
}

#[test]
fn renderer_uses_keep_disposal_when_static_pixels_can_persist() {
    let animation = moving_rect_over_opaque_bg_animation();
    let bytes = Renderer::default()
        .render_gif(&animation, GifRenderConfig::default())
        .unwrap();

    let (_canvas_width, _canvas_height, frames) = decode_gif_raw_frames(&bytes);

    assert!(
        frames
            .iter()
            .any(|frame| frame.dispose == ::gif::DisposalMethod::Keep)
    );
}

#[test]
fn renderer_crops_partial_gif_frames_when_only_a_region_changes() {
    let animation = moving_rect_over_opaque_bg_animation();
    let bytes = Renderer::default()
        .render_gif(&animation, GifRenderConfig::default())
        .unwrap();

    let (canvas_width, canvas_height, frames) = decode_gif_raw_frames(&bytes);

    assert!(
        frames.iter().any(|frame| {
            frame.raster.width < u32::from(canvas_width)
                || frame.raster.height < u32::from(canvas_height)
        }),
        "expected at least one partial GIF frame"
    );
}

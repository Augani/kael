use std::{
    iter,
    num::NonZeroUsize,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread,
};

use ::gif::{DisposalMethod, Encoder as GifEncoder, Repeat as GifRepeat};
use tiny_skia::Pixmap;

use super::{
    super::{
        composition::{new_pixmap, resolve_output_canvas_size},
        gif_encode::{GifSubframeRegion, encode_rgba_subframe},
        sample_cache::TimelineSampleCache,
    },
    GifRenderConfig, ImageAssetResolver, PreparedAnimation, PreparedResources, Renderer,
    ShapeCaches, StaticPathCache,
};
use crate::{Animation, RasterlottieError};

#[derive(Debug, Clone, Copy)]
struct ScheduledGifFrame {
    source_frame: f32,
    delay: u16,
}

struct EncodedGifFrame {
    index: usize,
    frame: ::gif::Frame<'static>,
}

struct RenderedGifFrame {
    index: usize,
    job: ScheduledGifFrame,
    pixels: Vec<u8>,
}

struct PlannedGifFrame {
    index: usize,
    left: u16,
    top: u16,
    width: u16,
    height: u16,
    delay: u16,
    dispose: DisposalMethod,
    rgba: Vec<u8>,
}

#[derive(Clone, Copy)]
struct GifFrameBounds {
    left: u16,
    top: u16,
    width: u16,
    height: u16,
}

#[derive(Clone, Copy)]
struct GifRenderPipeline<'a> {
    animation: &'a Animation,
    config: GifRenderConfig,
    resources: PreparedResources<'a>,
    width: u16,
    height: u16,
}

struct GifFrameJobList {
    #[cfg(feature = "tracing")]
    requested_fps: f32,
    #[cfg(feature = "tracing")]
    output_duration_seconds: f32,
    frames: Vec<ScheduledGifFrame>,
}

impl Renderer {
    /// Renders an animation as a GIF without retaining preprocessing state.
    ///
    /// # Errors
    ///
    /// Returns an error when preprocessing fails or GIF encoding cannot be completed.
    pub fn render_gif(
        &self,
        animation: &Animation,
        config: GifRenderConfig,
    ) -> Result<Vec<u8>, RasterlottieError> {
        self.prepare(animation)?.render_gif(config)
    }

    /// Renders an animation as a GIF while allowing external image asset resolution.
    ///
    /// # Errors
    ///
    /// Returns an error when preprocessing fails, asset resolution fails, or
    /// GIF encoding cannot be completed.
    pub fn render_gif_with_resolver<R: ImageAssetResolver>(
        &self,
        animation: &Animation,
        config: GifRenderConfig,
        resolver: &R,
    ) -> Result<Vec<u8>, RasterlottieError> {
        self.prepare_with_resolver(animation, resolver)?
            .render_gif(config)
    }

    pub(super) fn render_gif_with_assets(
        &self,
        animation: &Animation,
        config: GifRenderConfig,
        resources: PreparedResources<'_>,
    ) -> Result<Vec<u8>, RasterlottieError> {
        span_enter!(
            tracing::Level::TRACE,
            "render_gif_with_assets",
            width = animation.width,
            height = animation.height
        );
        let (output_width, output_height) = resolve_output_canvas_size(animation, config.render)?;
        let width =
            u16::try_from(output_width).map_err(|_| RasterlottieError::InvalidCanvasSize {
                width: output_width,
                height: output_height,
            })?;
        let height =
            u16::try_from(output_height).map_err(|_| RasterlottieError::InvalidCanvasSize {
                width: output_width,
                height: output_height,
            })?;
        let frame_jobs = build_gif_frame_jobs(animation, config);

        let mut bytes = Vec::new();
        let mut encoder = {
            span_enter!(
                tracing::Level::TRACE,
                "gif_encoder_init",
                width = width,
                height = height
            );
            #[cfg(feature = "tracing")]
            trace!(
                requested_fps = frame_jobs.requested_fps,
                output_duration_seconds = frame_jobs.output_duration_seconds,
                "gif frame schedule"
            );
            GifEncoder::new(&mut bytes, width, height, &[])?
        };
        encoder.set_repeat(GifRepeat::Infinite)?;
        let pipeline = GifRenderPipeline {
            animation,
            config,
            resources,
            width,
            height,
        };
        let worker_count = resolve_gif_worker_count(frame_jobs.frames.len());
        if worker_count <= 1 {
            self.render_gif_frames_sequential(pipeline, &frame_jobs.frames, &mut encoder)?;
        } else {
            self.render_gif_frames_parallel(
                pipeline,
                &frame_jobs.frames,
                worker_count,
                &mut encoder,
            )?;
        }

        drop(encoder);
        Ok(bytes)
    }

    #[cfg(test)]
    pub(super) fn render_gif_with_assets_for_test(
        &self,
        animation: &Animation,
        config: GifRenderConfig,
        resources: PreparedResources<'_>,
        worker_count: usize,
    ) -> Result<Vec<u8>, RasterlottieError> {
        span_enter!(
            tracing::Level::TRACE,
            "render_gif_with_assets",
            width = animation.width,
            height = animation.height
        );
        let (output_width, output_height) = resolve_output_canvas_size(animation, config.render)?;
        let width =
            u16::try_from(output_width).map_err(|_| RasterlottieError::InvalidCanvasSize {
                width: output_width,
                height: output_height,
            })?;
        let height =
            u16::try_from(output_height).map_err(|_| RasterlottieError::InvalidCanvasSize {
                width: output_width,
                height: output_height,
            })?;
        let frame_jobs = build_gif_frame_jobs(animation, config);

        let mut bytes = Vec::new();
        let mut encoder = GifEncoder::new(&mut bytes, width, height, &[])?;
        encoder.set_repeat(GifRepeat::Infinite)?;
        let pipeline = GifRenderPipeline {
            animation,
            config,
            resources,
            width,
            height,
        };
        if worker_count <= 1 {
            self.render_gif_frames_sequential(pipeline, &frame_jobs.frames, &mut encoder)?;
        } else {
            self.render_gif_frames_parallel(
                pipeline,
                &frame_jobs.frames,
                worker_count,
                &mut encoder,
            )?;
        }

        drop(encoder);
        Ok(bytes)
    }

    fn render_gif_frames_sequential(
        &self,
        pipeline: GifRenderPipeline<'_>,
        jobs: &[ScheduledGifFrame],
        encoder: &mut GifEncoder<&mut Vec<u8>>,
    ) -> Result<(), RasterlottieError> {
        let (output_width, output_height) =
            resolve_output_canvas_size(pipeline.animation, pipeline.config.render)?;
        let mut scratch = new_pixmap(output_width, output_height)?;
        let mut state_before = vec![0; output_width as usize * output_height as usize * 4];
        let mut current = None;

        for (frame_index, job) in jobs.iter().enumerate() {
            let next = self.render_raster_gif_frame(pipeline, &mut scratch, frame_index, *job)?;
            if let Some(previous) = current.take() {
                let planned = plan_gif_frame(
                    previous,
                    Some(next.pixels.as_slice()),
                    state_before.as_mut_slice(),
                    pipeline.width,
                    pipeline.height,
                );
                let gif_frame = encode_planned_gif_frame(
                    planned,
                    pipeline.width,
                    pipeline.config.color_quantizer_speed,
                );
                {
                    span_enter!(
                        tracing::Level::TRACE,
                        "write_gif_frame",
                        frame_index = frame_index - 1
                    );
                    encoder.write_frame(&gif_frame.frame)?;
                }
            }
            current = Some(next);
        }

        if let Some(last) = current {
            let planned = plan_gif_frame(
                last,
                None,
                state_before.as_mut_slice(),
                pipeline.width,
                pipeline.height,
            );
            let gif_frame = encode_planned_gif_frame(
                planned,
                pipeline.width,
                pipeline.config.color_quantizer_speed,
            );
            {
                span_enter!(
                    tracing::Level::TRACE,
                    "write_gif_frame",
                    frame_index = jobs.len().saturating_sub(1)
                );
                encoder.write_frame(&gif_frame.frame)?;
            }
        }

        Ok(())
    }

    fn render_gif_frames_parallel(
        &self,
        pipeline: GifRenderPipeline<'_>,
        jobs: &[ScheduledGifFrame],
        worker_count: usize,
        encoder: &mut GifEncoder<&mut Vec<u8>>,
    ) -> Result<(), RasterlottieError> {
        let (render_sender, render_receiver) =
            mpsc::channel::<Result<RenderedGifFrame, RasterlottieError>>();
        let (encode_job_sender, encode_job_receiver) = mpsc::channel::<PlannedGifFrame>();
        let (encode_sender, encode_receiver) =
            mpsc::channel::<Result<EncodedGifFrame, RasterlottieError>>();
        let stop = AtomicBool::new(false);
        let next_job = AtomicUsize::new(0);
        let use_static_path_cache = pipeline.resources.shape_caches.static_paths.is_some();
        let use_timeline_sample_cache = pipeline.resources.shape_caches.timeline_samples.is_some();
        let shape_plan_cache = pipeline.resources.shape_caches.plans;
        let layer_hierarchy_cache = pipeline.resources.layer_hierarchy_cache;
        let shared_encode_job_receiver = Arc::new(Mutex::new(encode_job_receiver));

        thread::scope(|scope| -> Result<(), RasterlottieError> {
            for _worker_index in 0..worker_count {
                let sender = render_sender.clone();
                let stop = &stop;
                let next_job = &next_job;
                let worker_image_assets = pipeline.resources.image_assets.clone_for_worker();
                scope.spawn(move || {
                    let result = (|| -> Result<(), RasterlottieError> {
                        let worker_static_path_cache =
                            use_static_path_cache.then(StaticPathCache::default);
                        let worker_timeline_sample_cache =
                            use_timeline_sample_cache.then(TimelineSampleCache::default);
                        let worker_resources = PreparedResources {
                            image_assets: &worker_image_assets,
                            shape_caches: ShapeCaches {
                                static_paths: worker_static_path_cache.as_ref(),
                                plans: shape_plan_cache,
                                timeline_samples: worker_timeline_sample_cache.as_ref(),
                            },
                            layer_hierarchy_cache,
                        };
                        let (output_width, output_height) =
                            resolve_output_canvas_size(pipeline.animation, pipeline.config.render)?;
                        let mut scratch = new_pixmap(output_width, output_height)?;
                        let worker_pipeline = GifRenderPipeline {
                            animation: pipeline.animation,
                            config: pipeline.config,
                            resources: worker_resources,
                            width: pipeline.width,
                            height: pipeline.height,
                        };

                        loop {
                            if stop.load(Ordering::Relaxed) {
                                break;
                            }

                            let frame_index = next_job.fetch_add(1, Ordering::Relaxed);
                            let Some(job) = jobs.get(frame_index).copied() else {
                                break;
                            };
                            let frame = self.render_raster_gif_frame(
                                worker_pipeline,
                                &mut scratch,
                                frame_index,
                                job,
                            )?;
                            if sender.send(Ok(frame)).is_err() {
                                break;
                            }
                        }

                        Ok(())
                    })();
                    if let Err(error) = result {
                        stop.store(true, Ordering::Relaxed);
                        let _ = sender.send(Err(error));
                    }
                });
            }

            for _worker_index in 0..worker_count {
                let receiver = Arc::clone(&shared_encode_job_receiver);
                let sender = encode_sender.clone();
                scope.spawn(move || {
                    loop {
                        let planned = if let Ok(receiver) = receiver.lock() {
                            receiver.recv()
                        } else {
                            let _ = sender.send(Err(RasterlottieError::Internal {
                                detail: "GIF encode worker receiver mutex was poisoned".to_string(),
                            }));
                            return;
                        };
                        let Ok(planned) = planned else {
                            break;
                        };
                        let encoded_frame = encode_planned_gif_frame(
                            planned,
                            pipeline.width,
                            pipeline.config.color_quantizer_speed,
                        );
                        if sender.send(Ok(encoded_frame)).is_err() {
                            break;
                        }
                    }
                });
            }
            drop(render_sender);
            drop(encode_sender);

            let mut state_before =
                vec![0; usize::from(pipeline.width) * usize::from(pipeline.height) * 4];
            let mut pending_rendered = iter::repeat_with(|| None)
                .take(jobs.len())
                .collect::<Vec<Option<RenderedGifFrame>>>();
            let mut current = None;
            let mut next_to_plan = 0usize;
            while next_to_plan < jobs.len() {
                let result = render_receiver
                    .recv()
                    .map_err(|_| RasterlottieError::Internal {
                        detail: "GIF render worker channel closed unexpectedly".to_string(),
                    })?;
                match result {
                    Ok(rendered_frame) => {
                        let frame_index = rendered_frame.index;
                        pending_rendered[frame_index] = Some(rendered_frame);
                        while next_to_plan < pending_rendered.len() {
                            let Some(next) = pending_rendered[next_to_plan].take() else {
                                break;
                            };
                            if let Some(previous) = current.take() {
                                let planned = plan_gif_frame(
                                    previous,
                                    Some(next.pixels.as_slice()),
                                    state_before.as_mut_slice(),
                                    pipeline.width,
                                    pipeline.height,
                                );
                                encode_job_sender.send(planned).map_err(|_| {
                                    RasterlottieError::Internal {
                                        detail: "GIF encode job channel closed unexpectedly"
                                            .to_string(),
                                    }
                                })?;
                            }
                            current = Some(next);
                            next_to_plan += 1;
                        }
                    }
                    Err(error) => {
                        stop.store(true, Ordering::Relaxed);
                        return Err(error);
                    }
                }
            }
            if let Some(last) = current {
                let planned = plan_gif_frame(
                    last,
                    None,
                    state_before.as_mut_slice(),
                    pipeline.width,
                    pipeline.height,
                );
                encode_job_sender
                    .send(planned)
                    .map_err(|_| RasterlottieError::Internal {
                        detail: "GIF encode job channel closed unexpectedly".to_string(),
                    })?;
            }
            drop(encode_job_sender);

            let mut pending_frames = iter::repeat_with(|| None)
                .take(jobs.len())
                .collect::<Vec<Option<::gif::Frame<'static>>>>();
            let mut next_to_write = 0usize;
            while next_to_write < jobs.len() {
                let result = encode_receiver
                    .recv()
                    .map_err(|_| RasterlottieError::Internal {
                        detail: "GIF worker channel closed unexpectedly".to_string(),
                    })?;
                match result {
                    Ok(encoded_frame) => {
                        pending_frames[encoded_frame.index] = Some(encoded_frame.frame);
                        while next_to_write < pending_frames.len() {
                            let Some(frame) = pending_frames[next_to_write].take() else {
                                break;
                            };
                            {
                                span_enter!(
                                    tracing::Level::TRACE,
                                    "write_gif_frame",
                                    frame_index = next_to_write
                                );
                                encoder.write_frame(&frame)?;
                            }
                            next_to_write += 1;
                        }
                    }
                    Err(error) => {
                        stop.store(true, Ordering::Relaxed);
                        return Err(error);
                    }
                }
            }

            Ok(())
        })
    }

    fn render_raster_gif_frame(
        &self,
        pipeline: GifRenderPipeline<'_>,
        scratch: &mut Pixmap,
        frame_index: usize,
        job: ScheduledGifFrame,
    ) -> Result<RenderedGifFrame, RasterlottieError> {
        span_enter!(
            tracing::Level::TRACE,
            "render_gif_frame",
            frame_index = frame_index,
            source_frame = job.source_frame
        );
        self.render_frame_with_assets_into(
            pipeline.animation,
            job.source_frame,
            pipeline.config.render,
            pipeline.resources,
            scratch,
        )?;

        Ok(RenderedGifFrame {
            index: frame_index,
            job,
            pixels: scratch.data().to_vec(),
        })
    }
}

impl PreparedAnimation {
    /// Renders the prepared animation as a GIF.
    ///
    /// # Errors
    ///
    /// Returns an error when the prepared animation cannot be encoded as a GIF.
    pub fn render_gif(&self, config: GifRenderConfig) -> Result<Vec<u8>, RasterlottieError> {
        self.renderer
            .render_gif_with_assets(&self.animation, config, self.prepared_resources())
    }

    #[cfg(test)]
    pub(crate) fn render_gif_with_parallelism_for_test(
        &self,
        config: GifRenderConfig,
        worker_count: usize,
    ) -> Result<Vec<u8>, RasterlottieError> {
        self.renderer.render_gif_with_assets_for_test(
            &self.animation,
            config,
            self.prepared_resources(),
            worker_count.max(1),
        )
    }
}

#[must_use]
fn build_gif_frame_jobs(animation: &Animation, config: GifRenderConfig) -> GifFrameJobList {
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
        frames.push(ScheduledGifFrame {
            source_frame,
            delay,
        });
    }

    GifFrameJobList {
        #[cfg(feature = "tracing")]
        requested_fps,
        #[cfg(feature = "tracing")]
        output_duration_seconds,
        frames,
    }
}

#[must_use]
fn resolve_gif_worker_count(frame_count: usize) -> usize {
    if frame_count < 2 {
        return 1;
    }

    thread::available_parallelism()
        .map_or(1, NonZeroUsize::get)
        .min(frame_count)
        .min(8)
}

fn plan_gif_frame(
    mut current: RenderedGifFrame,
    next_pixels: Option<&[u8]>,
    state_before: &mut [u8],
    width: u16,
    height: u16,
) -> PlannedGifFrame {
    span_enter!(
        tracing::Level::TRACE,
        "plan_gif_frame",
        frame_index = current.index
    );
    let base_bounds = diff_rgba_bounds(state_before, &current.pixels, width, height);
    let transparent_clear_bounds = next_pixels
        .and_then(|next| disappearing_to_transparent_bounds(&current.pixels, next, width, height));

    let (bounds, dispose) = if let Some(clear_bounds) = transparent_clear_bounds {
        (
            union_gif_frame_bounds(base_bounds, Some(clear_bounds)).unwrap_or(clear_bounds),
            DisposalMethod::Background,
        )
    } else if let Some(base_bounds) = base_bounds {
        (base_bounds, DisposalMethod::Keep)
    } else if next_pixels.is_none() {
        (
            alpha_visible_bounds(&current.pixels, width, height).unwrap_or(GifFrameBounds {
                left: 0,
                top: 0,
                width: 1,
                height: 1,
            }),
            DisposalMethod::Background,
        )
    } else {
        (
            GifFrameBounds {
                left: 0,
                top: 0,
                width: 1,
                height: 1,
            },
            DisposalMethod::Keep,
        )
    };

    current.job.delay = clamp_gif_delay(current.job.delay);
    state_before.copy_from_slice(&current.pixels);
    if matches!(dispose, DisposalMethod::Background) {
        clear_rgba_bounds(state_before, width, bounds);
    }

    PlannedGifFrame {
        index: current.index,
        left: bounds.left,
        top: bounds.top,
        width: bounds.width,
        height: bounds.height,
        delay: current.job.delay,
        dispose,
        rgba: current.pixels,
    }
}

fn encode_planned_gif_frame(
    planned: PlannedGifFrame,
    source_width: u16,
    quantizer_speed: i32,
) -> EncodedGifFrame {
    let mut rgba = planned.rgba;
    let mut frame = {
        span_enter!(
            tracing::Level::TRACE,
            "encode_gif_frame",
            frame_index = planned.index,
            quantizer_speed = quantizer_speed
        );
        encode_rgba_subframe(
            GifSubframeRegion {
                source_width,
                left: planned.left,
                top: planned.top,
                width: planned.width,
                height: planned.height,
            },
            &mut rgba,
            quantizer_speed,
        )
    };
    frame.left = planned.left;
    frame.top = planned.top;
    frame.delay = planned.delay;
    frame.dispose = planned.dispose;
    EncodedGifFrame {
        index: planned.index,
        frame,
    }
}

fn diff_rgba_bounds(
    previous: &[u8],
    current: &[u8],
    width: u16,
    height: u16,
) -> Option<GifFrameBounds> {
    bounded_rgba_changes(width, height, previous, current, rgba_pixels_differ)
}

fn disappearing_to_transparent_bounds(
    current: &[u8],
    next: &[u8],
    width: u16,
    height: u16,
) -> Option<GifFrameBounds> {
    bounded_rgba_changes(width, height, current, next, |current, next| {
        current[3] != 0 && next[3] == 0
    })
}

fn alpha_visible_bounds(rgba: &[u8], width: u16, height: u16) -> Option<GifFrameBounds> {
    bounded_rgba_changes(width, height, rgba, rgba, |current, _| current[3] != 0)
}

fn bounded_rgba_changes<F>(
    width: u16,
    height: u16,
    left: &[u8],
    right: &[u8],
    mut predicate: F,
) -> Option<GifFrameBounds>
where
    F: FnMut(&[u8], &[u8]) -> bool,
{
    let width = usize::from(width);
    let height = usize::from(height);
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0usize;
    let mut max_y = 0usize;
    let mut found = false;

    for (index, (left, right)) in left.chunks_exact(4).zip(right.chunks_exact(4)).enumerate() {
        if !predicate(left, right) {
            continue;
        }

        let x = index % width;
        let y = index / width;
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
        found = true;
    }

    found.then(|| GifFrameBounds {
        left: u16::try_from(min_x).unwrap_or(0),
        top: u16::try_from(min_y).unwrap_or(0),
        width: u16::try_from(max_x - min_x + 1).unwrap_or(1),
        height: u16::try_from(max_y - min_y + 1).unwrap_or(1),
    })
}

fn rgba_pixels_differ(left: &[u8], right: &[u8]) -> bool {
    if left[3] == 0 && right[3] == 0 {
        return false;
    }

    left != right
}

fn union_gif_frame_bounds(
    left: Option<GifFrameBounds>,
    right: Option<GifFrameBounds>,
) -> Option<GifFrameBounds> {
    match (left, right) {
        (Some(left), Some(right)) => {
            let left_max_x = u32::from(left.left) + u32::from(left.width) - 1;
            let left_max_y = u32::from(left.top) + u32::from(left.height) - 1;
            let right_max_x = u32::from(right.left) + u32::from(right.width) - 1;
            let right_max_y = u32::from(right.top) + u32::from(right.height) - 1;
            let min_x = left.left.min(right.left);
            let min_y = left.top.min(right.top);
            let max_x = left_max_x.max(right_max_x);
            let max_y = left_max_y.max(right_max_y);
            Some(GifFrameBounds {
                left: min_x,
                top: min_y,
                width: u16::try_from(max_x - u32::from(min_x) + 1).unwrap_or(u16::MAX),
                height: u16::try_from(max_y - u32::from(min_y) + 1).unwrap_or(u16::MAX),
            })
        }
        (Some(bounds), None) | (None, Some(bounds)) => Some(bounds),
        (None, None) => None,
    }
}

fn clear_rgba_bounds(rgba: &mut [u8], width: u16, bounds: GifFrameBounds) {
    let width = usize::from(width);
    let left = usize::from(bounds.left);
    let top = usize::from(bounds.top);
    let clear_width = usize::from(bounds.width);
    let clear_height = usize::from(bounds.height);

    for y in top..top + clear_height {
        let row_start = ((y * width) + left) * 4;
        let row_end = row_start + clear_width * 4;
        rgba[row_start..row_end].fill(0);
    }
}

const fn clamp_gif_delay(delay: u16) -> u16 {
    if delay == 0 { 1 } else { delay }
}

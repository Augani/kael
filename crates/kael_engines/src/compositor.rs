//! Timeline-driven compositing against the reference compositor — the preview==export
//! oracle (V1 + V5).
//!
//! [`composite_frame`] resolves which source frame each track shows at a given timeline
//! frame (via [`Timeline::frame_requests`]) and stacks the tracks bottom-to-top with
//! source-over alpha compositing. Source images come from a [`FrameProvider`]: the media
//! decoder in production, a synthetic generator in tests — so the full timeline→pixels
//! path is exercised without a decoder.

use std::collections::HashMap;

use kael_render_graph::reference::{blend, crossfade, BlendMode, Image, PassOp};
use kael_render_graph::{PassDesc, PassId, RenderGraph, ResourceDesc, ResourceId};

use crate::media::Timeline;

/// Supplies a decoded source image for a clip's source frame.
///
/// Returning `None` means the frame is unavailable, and that layer is skipped rather
/// than aborting the whole composite.
pub trait FrameProvider {
    /// Fetch the image for `source` at `source_frame`, sized `width` x `height`.
    fn frame(&self, source: &str, source_frame: u64, width: u32, height: u32) -> Option<Image>;
}

/// Composite the timeline at `frame` into a single `width` x `height` image.
///
/// Each track's active clip is fetched from `provider`, its effect stack is applied, and the
/// layers are stacked in track-declaration order (the first track is the bottom layer),
/// honoring the clip's opacity and blend mode. Two clips overlapping on a track crossfade as
/// one transition layer (see [`render_transition`]). Tracks with no active clip — or whose
/// source frame the provider cannot supply, or whose supplied image is the wrong size — are
/// skipped. The result is transparent where nothing is active. For timelines without clip
/// overlaps this matches the graph form built by [`build_frame_graph`] (run via
/// [`render_frames`]), so it remains the preview==export oracle there; the graph path does
/// not yet emit transition passes.
pub fn composite_frame(
    timeline: &Timeline,
    frame: u64,
    width: u32,
    height: u32,
    provider: &dyn FrameProvider,
) -> Image {
    let mut output = Image::new(width, height);
    for (index, track) in timeline.tracks.iter().enumerate() {
        if !track.enabled {
            continue;
        }
        // Two overlapping clips on a track crossfade as one transition layer; otherwise the
        // track contributes its single active clip. The single-clip path mirrors
        // `frame_requests` exactly, so timelines without overlaps composite identically.
        let (layer, mode) = if let Some(transition) =
            render_transition(timeline, index, frame, width, height, provider)
        {
            (transition, BlendMode::Normal)
        } else {
            let Some((clip, source_frame)) = track
                .clips
                .iter()
                .find_map(|clip| clip.source_frame_at(frame).map(|sf| (clip, sf)))
            else {
                continue;
            };
            let Some(image) = provider.frame(&clip.source, source_frame, width, height) else {
                continue;
            };
            if image.width != width || image.height != height {
                continue;
            }
            let clip_local = frame.saturating_sub(clip.track_offset);
            let time_ms = if timeline.frame_rate > 0.0 {
                (clip_local as f64 / timeline.frame_rate * 1000.0) as u64
            } else {
                0
            };
            let effected = clip.effects.resolve(time_ms).apply(&image);
            let mut layer = clip.transform.apply(&effected);
            let opacity = clip.opacity.clamp(0.0, 1.0);
            if opacity < 1.0 {
                for pixel in layer.pixels.iter_mut() {
                    pixel[3] *= opacity;
                }
            }
            (layer, clip.blend_mode.to_render_graph())
        };
        let mut next = Image::new(width, height);
        blend(mode)(&[&layer, &output], &mut next);
        output = next;
    }
    output
}

/// One composited layer in a [`FrameGraph`]: the source pass producing the clip's image
/// and the composite pass blending it over the layers below.
#[derive(Debug, Clone)]
pub struct LayerPass {
    /// The track this layer came from.
    pub track_id: String,
    /// The clip visible on that track at the requested frame.
    pub clip_id: String,
    /// The pass that produces the clip's source image.
    pub source: PassId,
    /// The clip's effect passes, applied in order between the source and the composite.
    pub effects: Vec<PassId>,
    /// The clip's geometric transform pass, if the transform is non-identity.
    pub transform: Option<PassId>,
    /// The pass that blends this layer over the accumulated result below it.
    pub composite: PassId,
}

/// The compositing render-graph for one timeline frame: the abstract pass DAG plus the
/// resource holding the final composited image.
///
/// This is the schedulable form of [`composite_frame`] — the same bottom-to-top stack
/// expressed as render-graph passes so a cache-aware executor
/// ([`execute_cached`](kael_render_graph::reference::execute_cached)) can skip layers whose
/// source frame and blend parameters are unchanged across frames. Each source pass carries
/// its clip's source frame as the pass frame PTS, so advancing the timeline invalidates
/// only the layers whose source frame actually changed.
#[derive(Debug)]
pub struct FrameGraph {
    /// The pass DAG: a background clear, then per-layer source + composite passes chained
    /// bottom-to-top.
    pub graph: RenderGraph,
    /// The resource holding the final composited frame (the cleared background when no
    /// layer is visible).
    pub output: ResourceId,
    /// The visible layers, bottom-to-top.
    pub layers: Vec<LayerPass>,
}

/// Build the compositing [`FrameGraph`] for `timeline` at `frame`.
///
/// Mirrors [`composite_frame`]'s bottom-to-top stack as render-graph passes: a cleared
/// background, then for each visible layer a source pass (tagged with the clip identity and
/// its source frame as the pass PTS) and a composite pass blending it over the accumulator.
/// Disabled tracks and tracks with no active clip are excluded (via
/// [`Timeline::frame_requests`]).
pub fn build_frame_graph(timeline: &Timeline, frame: u64) -> FrameGraph {
    let mut graph = RenderGraph::new();
    let background = graph.add_resource(ResourceDesc::transient_texture("background"));
    graph.add_pass(PassDesc::new("background").write(background));

    let mut accumulator = background;
    let mut layers = Vec::new();

    for (index, request) in timeline.frame_requests(frame).into_iter().enumerate() {
        let layer_texture =
            graph.add_resource(ResourceDesc::transient_texture(format!("layer_{index}")));
        let source = graph.add_pass(
            PassDesc::new(format!("source_{index}"))
                .write(layer_texture)
                .frame_pts(request.source_frame as i64)
                .param_hash(fnv1a(request.clip_id.as_bytes())),
        );

        // Chain the clip's effect stack between the source and the composite; each effect
        // pass reads the previous stage. The composite reads the last stage (the source
        // itself when the stack is empty). Cache keys fold the upstream key, so a changed
        // source frame invalidates the effects, and a changed effect param invalidates only
        // that effect and everything after it.
        let mut stage = layer_texture;
        let mut effects = Vec::new();
        for (effect_index, effect) in request.effects.effects.iter().enumerate() {
            let effect_texture = graph.add_resource(ResourceDesc::transient_texture(format!(
                "effect_{index}_{effect_index}"
            )));
            let effect_pass = graph.add_pass(
                PassDesc::new(format!("effect_{index}_{effect_index}"))
                    .read(stage)
                    .write(effect_texture)
                    .param_hash(effect.param_hash()),
            );
            effects.push(effect_pass);
            stage = effect_texture;
        }

        // A non-identity geometric transform is a pass after the effect chain.
        let transform = if request.transform.is_identity() {
            None
        } else {
            let transform_texture = graph.add_resource(ResourceDesc::transient_texture(format!(
                "transform_{index}"
            )));
            let transform_pass = graph.add_pass(
                PassDesc::new(format!("transform_{index}"))
                    .read(stage)
                    .write(transform_texture)
                    .param_hash(request.transform.param_hash()),
            );
            stage = transform_texture;
            Some(transform_pass)
        };

        let composited = graph.add_resource(ResourceDesc::transient_texture(format!(
            "composited_{index}"
        )));
        let composite = graph.add_pass(
            PassDesc::new(format!("composite_{index}"))
                .read(accumulator)
                .read(stage)
                .write(composited)
                .param_hash(blend_param(request.blend_mode as u8, request.opacity)),
        );

        layers.push(LayerPass {
            track_id: request.track_id,
            clip_id: request.clip_id,
            source,
            effects,
            transform,
            composite,
        });
        accumulator = composited;
    }

    FrameGraph {
        graph,
        output: accumulator,
        layers,
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn blend_param(blend_mode: u8, opacity: f32) -> u64 {
    let mut bytes = [0u8; 5];
    bytes[0] = blend_mode;
    bytes[1..].copy_from_slice(&opacity.to_le_bytes());
    fnv1a(&bytes)
}

/// Build the executable ops for `fg` so it can be run by
/// [`execute_cached`](kael_render_graph::reference::execute_cached) (or
/// [`execute`](kael_render_graph::reference::execute)).
///
/// Each source pass becomes an op writing the layer's decoded image (fetched from
/// `provider`); each effect pass applies its [`ClipEffect`](crate::effects::ClipEffect) op;
/// each composite pass applies the clip's opacity and blends the layer over the accumulator.
/// The background pass is intentionally left without an op so it clears to transparent.
/// Running the result composites the effect-applied layers, and with empty effect stacks it
/// matches [`composite_frame`] exactly — the preview==export oracle with per-layer caching.
/// A layer whose frame the provider cannot supply (or supplies at the wrong size) stays
/// transparent, matching `composite_frame`'s skip behavior.
pub fn frame_graph_ops(
    frame_graph: &FrameGraph,
    timeline: &Timeline,
    frame: u64,
    width: u32,
    height: u32,
    provider: &dyn FrameProvider,
) -> HashMap<PassId, PassOp<'static>> {
    let mut ops: HashMap<PassId, PassOp<'static>> = HashMap::new();
    for (layer, request) in frame_graph
        .layers
        .iter()
        .zip(timeline.frame_requests(frame))
    {
        let image = provider
            .frame(&request.source, request.source_frame, width, height)
            .filter(|fetched| fetched.width == width && fetched.height == height);
        ops.insert(layer.source, source_op(image));

        for (effect_pass, effect) in layer.effects.iter().zip(&request.effects.effects) {
            ops.insert(*effect_pass, effect.to_pass_op());
        }
        if let Some(transform_pass) = layer.transform {
            ops.insert(transform_pass, request.transform.to_pass_op());
        }

        let opacity = request.opacity.clamp(0.0, 1.0);
        ops.insert(
            layer.composite,
            composite_op(request.blend_mode.to_render_graph(), opacity),
        );
    }
    ops
}

fn source_op(image: Option<Image>) -> PassOp<'static> {
    Box::new(move |_inputs, output| {
        if let Some(image) = &image {
            if image.pixels.len() == output.pixels.len() {
                output.pixels.copy_from_slice(&image.pixels);
            }
        }
    })
}

fn composite_op(mode: BlendMode, opacity: f32) -> PassOp<'static> {
    let blend_op = blend(mode);
    Box::new(move |inputs, output| {
        if inputs.len() != 2 {
            return;
        }
        // The composite pass reads `[accumulator, layer]`; the reference `blend` expects
        // `[top, backdrop]`, so the layer is fed first with its opacity applied.
        let mut layer = inputs[1].clone();
        if opacity < 1.0 {
            for pixel in layer.pixels.iter_mut() {
                pixel[3] *= opacity;
            }
        }
        blend_op(&[&layer, inputs[0]], output);
    })
}

/// Render `timeline` over `frames` into a sequence of composited images.
///
/// Drives [`build_frame_graph`] + [`frame_graph_ops`] +
/// [`execute_cached`](kael_render_graph::reference::execute_cached) per frame, reusing one
/// render-graph cache across the whole range so passes whose cache key is unchanged (e.g.
/// the cleared background, or a still layer's source and effects) are not recomputed — V9
/// dirty-subtree caching applied to a range render. Each output frame equals
/// [`composite_frame`] for that frame when clips have no effects, and is the effect-applied
/// composite otherwise. This is the offscreen, window-independent timeline render the export
/// encoder consumes (P1-A) — the codec/mux stage is the only remaining gap.
pub fn render_frames(
    timeline: &Timeline,
    frames: impl IntoIterator<Item = u64>,
    width: u32,
    height: u32,
    provider: &dyn FrameProvider,
) -> Vec<Image> {
    let mut cache = HashMap::new();
    let mut rendered = Vec::new();
    for frame in frames {
        // The graph path does not yet emit transition passes, so frames with a clip overlap
        // are composited directly (still exact, just uncached); the rest — including clip
        // transforms, now graph passes — keep the cached graph path. This keeps
        // render_frames == composite_frame for every frame.
        let has_overlap =
            (0..timeline.tracks.len()).any(|index| timeline.transition_at(index, frame).is_some());
        if has_overlap {
            rendered.push(composite_frame(timeline, frame, width, height, provider));
            continue;
        }
        let frame_graph = build_frame_graph(timeline, frame);
        let Ok(compiled) = frame_graph.graph.compile() else {
            rendered.push(Image::new(width, height));
            continue;
        };
        let ops = frame_graph_ops(&frame_graph, timeline, frame, width, height, provider);
        let image = match kael_render_graph::reference::execute_cached(
            &frame_graph.graph,
            &compiled,
            width,
            height,
            &HashMap::new(),
            &ops,
            &mut cache,
        ) {
            Ok((images, _)) => images
                .get(&frame_graph.output)
                .cloned()
                .unwrap_or_else(|| Image::new(width, height)),
            Err(_) => Image::new(width, height),
        };
        rendered.push(image);
    }
    rendered
}

/// Render the crossfade transition on track `track_index` at `frame`, if two clips overlap
/// there (per [`Timeline::transition_at`]). Each side is fetched from `provider`, has its
/// (keyframed) effect stack applied at clip-local time, and the two are crossfaded by the
/// overlap progress. Returns `None` when there is no transition or a side cannot be supplied.
/// This renders a transition layer for the compositor to stack like any other.
pub fn render_transition(
    timeline: &Timeline,
    track_index: usize,
    frame: u64,
    width: u32,
    height: u32,
    provider: &dyn FrameProvider,
) -> Option<Image> {
    let transition = timeline.transition_at(track_index, frame)?;
    let track = timeline.tracks.get(track_index)?;
    let render_side = |clip_id: &str, source_frame: u64| -> Option<Image> {
        let clip = track
            .clips
            .iter()
            .find(|candidate| candidate.id == clip_id)?;
        let image = provider.frame(&clip.source, source_frame, width, height)?;
        if image.width != width || image.height != height {
            return None;
        }
        let clip_local = frame.saturating_sub(clip.track_offset);
        let time_ms = if timeline.frame_rate > 0.0 {
            (clip_local as f64 / timeline.frame_rate * 1000.0) as u64
        } else {
            0
        };
        let effected = clip.effects.resolve(time_ms).apply(&image);
        Some(clip.transform.apply(&effected))
    };
    let outgoing = render_side(&transition.outgoing, transition.outgoing_source_frame)?;
    let incoming = render_side(&transition.incoming, transition.incoming_source_frame)?;
    let mut output = Image::new(width, height);
    crossfade(transition.progress)(&[&outgoing, &incoming], &mut output);
    Some(output)
}

/// A [`FrameProvider`] serving a single decoded still image per source — a photo, freeze
/// frame, or title card. The concrete uncompressed-video provider, mirroring the WAV
/// audio provider. Returns the image when the requested size matches its own.
#[derive(Debug, Default)]
pub struct StillImageProvider {
    images: std::collections::HashMap<String, Image>,
}

impl StillImageProvider {
    /// An empty provider.
    pub fn new() -> Self {
        Self::default()
    }

    /// Decode a binary PPM and register it under `source`. Returns `false` if the bytes
    /// are not a decodable PPM.
    pub fn add_ppm(&mut self, source: impl Into<String>, ppm_bytes: &[u8]) -> bool {
        let Some(image) = crate::export::decode_ppm(ppm_bytes) else {
            return false;
        };
        self.images.insert(source.into(), image);
        true
    }

    /// Register an already-decoded image under `source`.
    pub fn add_image(&mut self, source: impl Into<String>, image: Image) {
        self.images.insert(source.into(), image);
    }
}

impl FrameProvider for StillImageProvider {
    fn frame(&self, source: &str, _source_frame: u64, width: u32, height: u32) -> Option<Image> {
        let image = self.images.get(source)?;
        (image.width == width && image.height == height).then(|| image.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::{Automation, Interpolation, Keyframe};
    use crate::effects::{AnimatedEffect, AnimatedEffectStack, ClipEffect, EffectStack};
    use crate::media::{ClipBlendMode, TimelineClip, TimelineTrack, TrackType};
    use crate::transform::ClipTransform;
    use std::collections::HashMap;

    struct SolidProvider {
        colors: HashMap<String, [f32; 4]>,
    }

    impl FrameProvider for SolidProvider {
        fn frame(
            &self,
            source: &str,
            _source_frame: u64,
            width: u32,
            height: u32,
        ) -> Option<Image> {
            self.colors
                .get(source)
                .map(|&color| Image::filled(width, height, color))
        }
    }

    fn clip(id: &str, source: &str, start: u64, end: u64, offset: u64) -> TimelineClip {
        TimelineClip {
            id: id.to_string(),
            source: source.to_string(),
            start_frame: start,
            end_frame: end,
            track_offset: offset,
            opacity: 1.0,
            opacity_curve: None,
            blend_mode: ClipBlendMode::Normal,
            effects: Default::default(),
            transform: Default::default(),
        }
    }

    fn track(id: &str, clips: Vec<TimelineClip>) -> TimelineTrack {
        TimelineTrack {
            id: id.to_string(),
            name: id.to_string(),
            track_type: TrackType::Video,
            clips,
            enabled: true,
        }
    }

    fn provider(pairs: &[(&str, [f32; 4])]) -> SolidProvider {
        SolidProvider {
            colors: pairs
                .iter()
                .map(|(source, color)| (source.to_string(), *color))
                .collect(),
        }
    }

    #[test]
    fn build_frame_graph_has_a_layer_per_visible_track() {
        let timeline = Timeline {
            tracks: vec![
                track("v1", vec![clip("a", "red", 0, 30, 0)]),
                track("v2", vec![clip("b", "blue", 0, 30, 0)]),
            ],
            frame_rate: 30.0,
            duration_frames: 30,
        };
        let fg = build_frame_graph(&timeline, 10);
        assert_eq!(fg.layers.len(), 2);

        let compiled = fg.graph.compile().expect("frame graph compiles");
        let order = compiled.execution_order();
        for layer in &fg.layers {
            let source_pos = order.iter().position(|&p| p == layer.source).unwrap();
            let composite_pos = order.iter().position(|&p| p == layer.composite).unwrap();
            assert!(
                source_pos < composite_pos,
                "source must precede its composite"
            );
        }
    }

    #[test]
    fn build_frame_graph_excludes_disabled_tracks() {
        let mut timeline = Timeline {
            tracks: vec![
                track("v1", vec![clip("a", "red", 0, 30, 0)]),
                track("v2", vec![clip("b", "blue", 0, 30, 0)]),
            ],
            frame_rate: 30.0,
            duration_frames: 30,
        };
        timeline.tracks[0].enabled = false;
        let fg = build_frame_graph(&timeline, 10);
        assert_eq!(fg.layers.len(), 1);
        assert_eq!(fg.layers[0].track_id, "v2");
    }

    #[test]
    fn build_frame_graph_cache_keys_track_the_source_frame() {
        let timeline = Timeline {
            tracks: vec![track("v1", vec![clip("a", "red", 0, 60, 0)])],
            frame_rate: 30.0,
            duration_frames: 60,
        };
        let key_at = |frame: u64| {
            let fg = build_frame_graph(&timeline, frame);
            let compiled = fg.graph.compile().unwrap();
            compiled.cache_key(fg.layers[0].source).unwrap()
        };
        // Deterministic at a fixed frame; invalidated as the source frame advances.
        assert_eq!(key_at(10), key_at(10));
        assert_ne!(key_at(10), key_at(20));
    }

    #[test]
    fn frame_graph_execution_matches_composite_frame() {
        let mut timeline = Timeline {
            tracks: vec![
                track("v1", vec![clip("a", "red", 0, 30, 0)]),
                track("v2", vec![clip("b", "blue", 0, 30, 0)]),
            ],
            frame_rate: 30.0,
            duration_frames: 30,
        };
        // Exercise an effect stack, opacity, and a non-trivial blend mode on the top layer
        // so the equivalence covers the source -> effects -> opacity -> blend path.
        timeline.tracks[1].clips[0].effects = EffectStack {
            effects: vec![ClipEffect::Exposure { stops: -1.0 }],
        }
        .into();
        timeline.tracks[1].clips[0].opacity = 0.5;
        timeline.tracks[1].clips[0].blend_mode = ClipBlendMode::Multiply;

        let provider = provider(&[
            ("red", [1.0, 0.0, 0.0, 1.0]),
            ("blue", [0.0, 0.0, 1.0, 1.0]),
        ]);
        let direct = composite_frame(&timeline, 10, 4, 4, &provider);

        let fg = build_frame_graph(&timeline, 10);
        let compiled = fg.graph.compile().unwrap();
        let ops = frame_graph_ops(&fg, &timeline, 10, 4, 4, &provider);
        let mut cache = HashMap::new();
        let (images, _) = kael_render_graph::reference::execute_cached(
            &fg.graph,
            &compiled,
            4,
            4,
            &HashMap::new(),
            &ops,
            &mut cache,
        )
        .unwrap();

        assert_eq!(images[&fg.output].pixels, direct.pixels);
    }

    #[test]
    fn frame_graph_ops_cover_source_and_composite_passes() {
        let timeline = Timeline {
            tracks: vec![
                track("v1", vec![clip("a", "red", 0, 30, 0)]),
                track("v2", vec![clip("b", "blue", 0, 30, 0)]),
            ],
            frame_rate: 30.0,
            duration_frames: 30,
        };
        let provider = provider(&[
            ("red", [1.0, 0.0, 0.0, 1.0]),
            ("blue", [0.0, 0.0, 1.0, 1.0]),
        ]);
        let fg = build_frame_graph(&timeline, 10);
        let ops = frame_graph_ops(&fg, &timeline, 10, 4, 4, &provider);
        for layer in &fg.layers {
            assert!(ops.contains_key(&layer.source));
            assert!(ops.contains_key(&layer.composite));
        }
        assert_eq!(ops.len(), fg.layers.len() * 2);
    }

    #[test]
    fn build_frame_graph_chains_clip_effect_passes_in_order() {
        let mut timeline = Timeline {
            tracks: vec![track("v1", vec![clip("a", "red", 0, 30, 0)])],
            frame_rate: 30.0,
            duration_frames: 30,
        };
        timeline.tracks[0].clips[0].effects = EffectStack {
            effects: vec![
                ClipEffect::Exposure { stops: 1.0 },
                ClipEffect::Gamma { power: 2.0 },
            ],
        }
        .into();
        let fg = build_frame_graph(&timeline, 10);
        assert_eq!(fg.layers[0].effects.len(), 2);

        let compiled = fg.graph.compile().expect("frame graph compiles");
        let order = compiled.execution_order();
        let pos = |p| order.iter().position(|&q| q == p).unwrap();
        let layer = &fg.layers[0];
        assert!(pos(layer.source) < pos(layer.effects[0]));
        assert!(pos(layer.effects[0]) < pos(layer.effects[1]));
        assert!(pos(layer.effects[1]) < pos(layer.composite));
    }

    #[test]
    fn build_frame_graph_emits_a_transform_pass() {
        let mut timeline = Timeline {
            tracks: vec![track("v1", vec![clip("a", "white", 0, 30, 0)])],
            frame_rate: 30.0,
            duration_frames: 30,
        };
        // No transform -> no transform pass.
        assert!(build_frame_graph(&timeline, 10).layers[0]
            .transform
            .is_none());

        // A non-identity transform -> a pass between the source and the composite.
        timeline.tracks[0].clips[0].transform = ClipTransform {
            scale_x: 0.5,
            scale_y: 0.5,
            center_x: 0.5,
            center_y: 0.5,
            rotation: 0.0,
        };
        let fg = build_frame_graph(&timeline, 10);
        let layer = &fg.layers[0];
        let transform = layer.transform.expect("transform pass present");
        let compiled = fg.graph.compile().expect("compiles");
        let order = compiled.execution_order();
        let pos = |p| order.iter().position(|&q| q == p).unwrap();
        assert!(pos(layer.source) < pos(transform));
        assert!(pos(transform) < pos(layer.composite));
    }

    #[test]
    fn frame_graph_applies_clip_effects_during_execution() {
        let mut timeline = Timeline {
            tracks: vec![track("v1", vec![clip("a", "gray", 0, 30, 0)])],
            frame_rate: 30.0,
            duration_frames: 30,
        };
        timeline.tracks[0].clips[0].effects = EffectStack {
            effects: vec![ClipEffect::Exposure { stops: 1.0 }],
        }
        .into();
        let provider = provider(&[("gray", [0.3, 0.3, 0.3, 1.0])]);

        let fg = build_frame_graph(&timeline, 10);
        let compiled = fg.graph.compile().unwrap();
        let ops = frame_graph_ops(&fg, &timeline, 10, 2, 2, &provider);
        let mut cache = HashMap::new();
        let (images, _) = kael_render_graph::reference::execute_cached(
            &fg.graph,
            &compiled,
            2,
            2,
            &HashMap::new(),
            &ops,
            &mut cache,
        )
        .unwrap();

        // Exposure +1 doubles the source (0.3 -> 0.6) before it composites over transparent.
        let out = images[&fg.output].pixel(0, 0);
        assert!((out[0] - 0.6).abs() < 1e-6, "{out:?}");
    }

    #[test]
    fn render_frames_matches_composite_frame_across_a_range() {
        let timeline = Timeline {
            tracks: vec![
                track("v1", vec![clip("a", "red", 0, 30, 0)]),
                // Starts later on the timeline, so early frames have one layer, later two.
                track("v2", vec![clip("b", "blue", 5, 30, 10)]),
            ],
            frame_rate: 30.0,
            duration_frames: 30,
        };
        let provider = provider(&[
            ("red", [1.0, 0.0, 0.0, 1.0]),
            ("blue", [0.0, 0.0, 1.0, 0.5]),
        ]);

        let frames = render_frames(&timeline, 0..15, 4, 4, &provider);
        assert_eq!(frames.len(), 15);
        for frame in 0..15u64 {
            let direct = composite_frame(&timeline, frame, 4, 4, &provider);
            assert_eq!(
                frames[frame as usize].pixels, direct.pixels,
                "frame {frame}"
            );
        }
    }

    #[test]
    fn render_frames_applies_clip_effects_every_frame() {
        let mut timeline = Timeline {
            tracks: vec![track("v1", vec![clip("a", "gray", 0, 30, 0)])],
            frame_rate: 30.0,
            duration_frames: 30,
        };
        timeline.tracks[0].clips[0].effects = EffectStack {
            effects: vec![ClipEffect::Exposure { stops: 1.0 }],
        }
        .into();
        let provider = provider(&[("gray", [0.3, 0.3, 0.3, 1.0])]);

        let frames = render_frames(&timeline, 0..3, 2, 2, &provider);
        assert_eq!(frames.len(), 3);
        for image in &frames {
            assert!(
                (image.pixel(0, 0)[0] - 0.6).abs() < 1e-6,
                "{:?}",
                image.pixel(0, 0)
            );
        }
    }

    #[test]
    fn composite_frame_applies_clip_effects() {
        let mut timeline = Timeline {
            tracks: vec![track("v1", vec![clip("a", "gray", 0, 30, 0)])],
            frame_rate: 30.0,
            duration_frames: 30,
        };
        let provider = provider(&[("gray", [0.3, 0.3, 0.3, 1.0])]);

        // Without effects the layer composites at its source value.
        let plain = composite_frame(&timeline, 10, 2, 2, &provider);
        assert!((plain.pixel(0, 0)[0] - 0.3).abs() < 1e-6);

        // With an exposure effect, composite_frame (the export path) reflects it.
        timeline.tracks[0].clips[0].effects = EffectStack {
            effects: vec![ClipEffect::Exposure { stops: 1.0 }],
        }
        .into();
        let graded = composite_frame(&timeline, 10, 2, 2, &provider);
        assert!(
            (graded.pixel(0, 0)[0] - 0.6).abs() < 1e-6,
            "{:?}",
            graded.pixel(0, 0)
        );
    }

    #[test]
    fn keyframed_clip_effect_animates_across_frames() {
        let mut timeline = Timeline {
            tracks: vec![track("v1", vec![clip("a", "gray", 0, 60, 0)])],
            frame_rate: 30.0,
            duration_frames: 60,
        };
        // Exposure ramps 0 -> +1 stop over the first second (clip-local), resolved per frame.
        let mut stops = Automation::constant(0.0);
        stops.add(Keyframe {
            time_ms: 0,
            value: 0.0,
            interpolation: Interpolation::Linear,
        });
        stops.add(Keyframe {
            time_ms: 1000,
            value: 1.0,
            interpolation: Interpolation::Linear,
        });
        timeline.tracks[0].clips[0].effects = AnimatedEffectStack {
            effects: vec![AnimatedEffect::Exposure { stops }],
        };
        let provider = provider(&[("gray", [0.3, 0.3, 0.3, 1.0])]);

        let value_at = |frame| composite_frame(&timeline, frame, 1, 1, &provider).pixel(0, 0)[0];
        // Frame 0 (0 ms): exposure 0, unchanged. Frame 30 (1000 ms): +1 stop, doubled.
        assert!((value_at(0) - 0.3).abs() < 1e-4, "{}", value_at(0));
        assert!((value_at(30) - 0.6).abs() < 1e-4, "{}", value_at(30));
        // The ramp is monotonic in between.
        assert!(value_at(0) < value_at(15) && value_at(15) < value_at(30));
    }

    #[test]
    fn composite_frame_applies_clip_transform() {
        let mut timeline = Timeline {
            tracks: vec![track("v1", vec![clip("a", "white", 0, 30, 0)])],
            frame_rate: 30.0,
            duration_frames: 30,
        };
        // A half-size, centered picture-in-picture.
        timeline.tracks[0].clips[0].transform = ClipTransform {
            scale_x: 0.5,
            scale_y: 0.5,
            center_x: 0.5,
            center_y: 0.5,
            rotation: 0.0,
        };
        let provider = provider(&[("white", [1.0, 1.0, 1.0, 1.0])]);
        let out = composite_frame(&timeline, 10, 4, 4, &provider);
        // The PiP box ([1,3) x [1,3)) is opaque; the borders are transparent.
        assert_eq!(out.pixel(2, 2), [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(out.pixel(0, 0)[3], 0.0);
    }

    #[test]
    fn render_frames_matches_composite_frame_with_a_transform() {
        let mut timeline = Timeline {
            tracks: vec![track("v1", vec![clip("a", "white", 0, 30, 0)])],
            frame_rate: 30.0,
            duration_frames: 30,
        };
        timeline.tracks[0].clips[0].transform = ClipTransform {
            scale_x: 0.5,
            scale_y: 0.5,
            center_x: 0.5,
            center_y: 0.5,
            rotation: 0.0,
        };
        let provider = provider(&[("white", [1.0, 1.0, 1.0, 1.0])]);
        let frames = render_frames(&timeline, 0..3, 4, 4, &provider);
        for (offset, frame) in (0..3u64).enumerate() {
            assert_eq!(
                frames[offset].pixels,
                composite_frame(&timeline, frame, 4, 4, &provider).pixels,
                "frame {frame}"
            );
        }
    }

    #[test]
    fn render_transition_crossfades_overlapping_clips() {
        // "a" [0,30) red and "b" [20,50) blue overlap on [20,30).
        let timeline = Timeline {
            tracks: vec![track(
                "v1",
                vec![clip("a", "red", 0, 30, 0), clip("b", "blue", 0, 30, 20)],
            )],
            frame_rate: 30.0,
            duration_frames: 50,
        };
        let provider = provider(&[
            ("red", [1.0, 0.0, 0.0, 1.0]),
            ("blue", [0.0, 0.0, 1.0, 1.0]),
        ]);

        // Midway (frame 25, progress 0.5): half red, half blue.
        let mid = render_transition(&timeline, 0, 25, 1, 1, &provider).unwrap();
        let pixel = mid.pixel(0, 0);
        assert!((pixel[0] - 0.5).abs() < 1e-6 && (pixel[2] - 0.5).abs() < 1e-6);

        // Overlap start is fully outgoing (red); near the end mostly incoming (blue).
        assert!(
            (render_transition(&timeline, 0, 20, 1, 1, &provider)
                .unwrap()
                .pixel(0, 0)[0]
                - 1.0)
                .abs()
                < 1e-6
        );
        assert!(
            render_transition(&timeline, 0, 29, 1, 1, &provider)
                .unwrap()
                .pixel(0, 0)[2]
                > 0.8
        );

        // No overlap at frame 10 -> no transition.
        assert!(render_transition(&timeline, 0, 10, 1, 1, &provider).is_none());
    }

    #[test]
    fn composite_frame_crossfades_a_transition() {
        let timeline = Timeline {
            tracks: vec![track(
                "v1",
                vec![clip("a", "red", 0, 30, 0), clip("b", "blue", 0, 30, 20)],
            )],
            frame_rate: 30.0,
            duration_frames: 50,
        };
        let provider = provider(&[
            ("red", [1.0, 0.0, 0.0, 1.0]),
            ("blue", [0.0, 0.0, 1.0, 1.0]),
        ]);

        // Outside the overlap the single active clip composites as before.
        assert_eq!(
            composite_frame(&timeline, 10, 1, 1, &provider).pixel(0, 0),
            [1.0, 0.0, 0.0, 1.0]
        );
        // Inside the overlap (frame 25) the two clips crossfade -> half red, half blue.
        let mid = composite_frame(&timeline, 25, 1, 1, &provider).pixel(0, 0);
        assert!(
            (mid[0] - 0.5).abs() < 1e-6 && (mid[2] - 0.5).abs() < 1e-6,
            "{mid:?}"
        );
    }

    #[test]
    fn render_frames_matches_composite_frame_through_a_transition() {
        let timeline = Timeline {
            tracks: vec![track(
                "v1",
                vec![clip("a", "red", 0, 30, 0), clip("b", "blue", 0, 30, 20)],
            )],
            frame_rate: 30.0,
            duration_frames: 50,
        };
        let provider = provider(&[
            ("red", [1.0, 0.0, 0.0, 1.0]),
            ("blue", [0.0, 0.0, 1.0, 1.0]),
        ]);

        // The range 18..32 spans the overlap [20,30); render_frames must equal
        // composite_frame on every frame, transition frames included.
        let frames = render_frames(&timeline, 18..32, 1, 1, &provider);
        for (offset, frame) in (18..32u64).enumerate() {
            assert_eq!(
                frames[offset].pixels,
                composite_frame(&timeline, frame, 1, 1, &provider).pixels,
                "frame {frame}"
            );
        }
    }

    #[test]
    fn composites_single_opaque_track() {
        let timeline = Timeline {
            tracks: vec![track("v1", vec![clip("a", "red", 0, 30, 0)])],
            frame_rate: 30.0,
            duration_frames: 30,
        };
        let out = composite_frame(
            &timeline,
            10,
            2,
            2,
            &provider(&[("red", [1.0, 0.0, 0.0, 1.0])]),
        );
        assert!(out
            .pixels
            .iter()
            .all(|pixel| *pixel == [1.0, 0.0, 0.0, 1.0]));
    }

    #[test]
    fn upper_track_composites_over_lower() {
        let timeline = Timeline {
            tracks: vec![
                track("v1", vec![clip("a", "red", 0, 30, 0)]),
                track("v2", vec![clip("b", "blue50", 0, 30, 0)]),
            ],
            frame_rate: 30.0,
            duration_frames: 30,
        };
        let provider = provider(&[
            ("red", [1.0, 0.0, 0.0, 1.0]),
            ("blue50", [0.0, 0.0, 1.0, 0.5]),
        ]);
        let pixel = composite_frame(&timeline, 5, 1, 1, &provider).pixel(0, 0);
        // Semi-transparent blue over opaque red: out_a=1, rgb = blue*0.5 + red*0.5.
        assert!((pixel[0] - 0.5).abs() < 1e-5, "red {}", pixel[0]);
        assert!((pixel[2] - 0.5).abs() < 1e-5, "blue {}", pixel[2]);
        assert!((pixel[3] - 1.0).abs() < 1e-5, "alpha {}", pixel[3]);
    }

    #[test]
    fn source_frame_passed_to_provider_is_frame_accurate() {
        // Clip source [100,110) placed at track offset 50; at timeline frame 53 the
        // provider must be asked for source frame 103.
        struct RecordingProvider {
            requested: std::cell::Cell<u64>,
        }
        impl FrameProvider for RecordingProvider {
            fn frame(&self, _s: &str, source_frame: u64, w: u32, h: u32) -> Option<Image> {
                self.requested.set(source_frame);
                Some(Image::new(w, h))
            }
        }
        let timeline = Timeline {
            tracks: vec![track("v1", vec![clip("a", "s", 100, 110, 50)])],
            frame_rate: 30.0,
            duration_frames: 200,
        };
        let provider = RecordingProvider {
            requested: std::cell::Cell::new(0),
        };
        composite_frame(&timeline, 53, 1, 1, &provider);
        assert_eq!(provider.requested.get(), 103);
    }

    #[test]
    fn empty_timeline_frame_is_transparent() {
        let timeline = Timeline {
            tracks: vec![],
            frame_rate: 30.0,
            duration_frames: 0,
        };
        let out = composite_frame(&timeline, 0, 2, 2, &provider(&[]));
        assert!(out.pixels.iter().all(|pixel| pixel[3] == 0.0));
    }

    #[test]
    fn missing_provider_frame_is_skipped() {
        let timeline = Timeline {
            tracks: vec![track("v1", vec![clip("a", "absent", 0, 30, 0)])],
            frame_rate: 30.0,
            duration_frames: 30,
        };
        let out = composite_frame(&timeline, 10, 2, 2, &provider(&[]));
        assert!(out.pixels.iter().all(|pixel| pixel[3] == 0.0));
    }

    #[test]
    fn clip_opacity_attenuates_layer_alpha() {
        let mut faded = clip("a", "red", 0, 30, 0);
        faded.opacity = 0.5;
        let timeline = Timeline {
            tracks: vec![track("v1", vec![faded])],
            frame_rate: 30.0,
            duration_frames: 30,
        };
        let pixel = composite_frame(
            &timeline,
            5,
            1,
            1,
            &provider(&[("red", [1.0, 0.0, 0.0, 1.0])]),
        )
        .pixel(0, 0);
        assert!((pixel[3] - 0.5).abs() < 1e-5, "alpha {}", pixel[3]);
        assert!((pixel[0] - 1.0).abs() < 1e-5, "red preserved {}", pixel[0]);
    }

    #[test]
    fn clip_blend_mode_multiplies_lower_track() {
        let mut top = clip("b", "gray", 0, 30, 0);
        top.blend_mode = ClipBlendMode::Multiply;
        let timeline = Timeline {
            tracks: vec![
                track("v1", vec![clip("a", "gray", 0, 30, 0)]),
                track("v2", vec![top]),
            ],
            frame_rate: 30.0,
            duration_frames: 30,
        };
        let pixel = composite_frame(
            &timeline,
            5,
            1,
            1,
            &provider(&[("gray", [0.5, 0.5, 0.5, 1.0])]),
        )
        .pixel(0, 0);
        // 0.5 multiplied by 0.5 = 0.25.
        assert!(
            (pixel[0] - 0.25).abs() < 1e-5,
            "multiply -> 0.25, got {}",
            pixel[0]
        );
    }

    #[test]
    fn still_image_provider_decodes_ppm_into_the_composite() {
        use crate::export::encode_ppm;
        use kael_render_graph::reference::Image;
        let ppm = encode_ppm(&Image::filled(2, 2, [1.0, 0.0, 0.0, 1.0]));
        let mut still = StillImageProvider::new();
        assert!(still.add_ppm("photo.ppm", &ppm));
        assert!(!still.add_ppm("bad", b"not ppm"));

        let timeline = Timeline {
            tracks: vec![track("v1", vec![clip("a", "photo.ppm", 0, 30, 0)])],
            frame_rate: 30.0,
            duration_frames: 30,
        };
        let pixel = composite_frame(&timeline, 10, 2, 2, &still).pixel(0, 0);
        assert!(
            (pixel[0] - 1.0).abs() < 1e-6 && pixel[1].abs() < 1e-6 && pixel[2].abs() < 1e-6,
            "decoded red still: {pixel:?}"
        );
    }
}

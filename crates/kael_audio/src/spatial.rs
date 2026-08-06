//! Lightweight spatial-audio scene and stereo source processing.

use std::{collections::HashMap, sync::Arc};

use anyhow::Result;
use parking_lot::Mutex;

use crate::SampleSource;

const MAX_SPATIAL_SOURCES: usize = 4 * 1024;

/// Identifier for a source registered with a [`SpatialAudioScene`].
pub type SpatialSourceId = u64;

/// A bounded spatial-audio scene that can pan and attenuate mixer sources.
///
/// This is an inexpensive stereo positional model, not an HRTF renderer. It applies equal-power
/// left/right panning and inverse-distance attenuation while keeping scene mutation off the audio
/// callback. When the scene lock is briefly busy, a spatialized source reuses its last gains.
#[derive(Clone, Debug, Default)]
pub struct SpatialAudioScene {
    inner: Arc<Mutex<SpatialState>>,
}

#[derive(Debug)]
struct SpatialState {
    listener_position: [f32; 3],
    listener_forward: [f32; 3],
    listener_up: [f32; 3],
    next_source_id: SpatialSourceId,
    source_positions: HashMap<SpatialSourceId, [f32; 3]>,
}

impl Default for SpatialState {
    fn default() -> Self {
        Self {
            listener_position: [0.0, 0.0, 0.0],
            listener_forward: [0.0, 0.0, -1.0],
            listener_up: [0.0, 1.0, 0.0],
            next_source_id: 1,
            source_positions: HashMap::new(),
        }
    }
}

impl SpatialAudioScene {
    /// Set the listener position.
    pub fn set_listener_position(&self, position: [f32; 3]) {
        self.inner.lock().listener_position = finite_position(position);
    }

    /// Return the current listener position.
    pub fn listener_position(&self) -> [f32; 3] {
        self.inner.lock().listener_position
    }

    /// Set an orthonormal listener orientation.
    pub fn set_listener_orientation(&self, forward: [f32; 3], up: [f32; 3]) {
        let forward = normalized(finite_position(forward), [0.0, 0.0, -1.0]);
        let fallback_axis = if forward[1].abs() < 0.9 {
            [0.0, 1.0, 0.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        let up = normalized(finite_position(up), fallback_axis);
        let projection = dot(up, forward);
        let orthogonal = [
            up[0] - projection * forward[0],
            up[1] - projection * forward[1],
            up[2] - projection * forward[2],
        ];
        let fallback_projection = dot(fallback_axis, forward);
        let fallback_up = normalized(
            [
                fallback_axis[0] - fallback_projection * forward[0],
                fallback_axis[1] - fallback_projection * forward[1],
                fallback_axis[2] - fallback_projection * forward[2],
            ],
            [0.0, 1.0, 0.0],
        );
        let up = normalized(orthogonal, fallback_up);
        let mut state = self.inner.lock();
        state.listener_forward = forward;
        state.listener_up = up;
    }

    /// Return the listener's normalized forward and up vectors.
    pub fn listener_orientation(&self) -> ([f32; 3], [f32; 3]) {
        let state = self.inner.lock();
        (state.listener_forward, state.listener_up)
    }

    /// Add a spatial source and return its scene-local identifier.
    pub fn add_source(&self, position: [f32; 3]) -> Result<SpatialSourceId> {
        let mut state = self.inner.lock();
        anyhow::ensure!(
            state.source_positions.len() < MAX_SPATIAL_SOURCES,
            "spatial audio scene exceeds the {MAX_SPATIAL_SOURCES}-source limit"
        );
        state
            .source_positions
            .try_reserve(1)
            .map_err(|_| anyhow::anyhow!("failed to reserve spatial audio source state"))?;
        let id = allocate_source_id(&mut state)?;
        state.source_positions.insert(id, finite_position(position));
        Ok(id)
    }

    /// Update a registered source position. Returns `false` for an unknown id.
    pub fn set_source_position(&self, id: SpatialSourceId, position: [f32; 3]) -> bool {
        let mut state = self.inner.lock();
        let Some(stored) = state.source_positions.get_mut(&id) else {
            return false;
        };
        *stored = finite_position(position);
        true
    }

    /// Return a registered source position.
    pub fn source_position(&self, id: SpatialSourceId) -> Option<[f32; 3]> {
        self.inner.lock().source_positions.get(&id).copied()
    }

    /// Remove a registered spatial source.
    ///
    /// Existing wrappers retain their last computed gains instead of jumping to
    /// full volume; new wrappers cannot be created after removal.
    pub fn remove_source(&self, id: SpatialSourceId) -> bool {
        self.inner.lock().source_positions.remove(&id).is_some()
    }

    /// Return the number of registered spatial sources.
    pub fn source_count(&self) -> usize {
        self.inner.lock().source_positions.len()
    }

    /// Return the current equal-power stereo gains for a registered source.
    pub fn stereo_gains(&self, id: SpatialSourceId) -> Option<[f32; 2]> {
        spatial_gains(&self.inner.lock(), id)
    }

    /// Wrap a mixer source with dynamic stereo panning and distance attenuation.
    pub fn spatialize(
        &self,
        id: SpatialSourceId,
        source: Box<dyn SampleSource>,
    ) -> Result<Box<dyn SampleSource>> {
        let last_gains = spatial_gains(&self.inner.lock(), id)
            .ok_or_else(|| anyhow::anyhow!("cannot spatialize an unknown source id"))?;
        Ok(Box::new(SpatializedSource {
            source,
            scene: self.inner.clone(),
            source_id: id,
            last_gains,
        }))
    }
}

struct SpatializedSource {
    source: Box<dyn SampleSource>,
    scene: Arc<Mutex<SpatialState>>,
    source_id: SpatialSourceId,
    last_gains: [f32; 2],
}

impl SampleSource for SpatializedSource {
    fn fill(&mut self, output: &mut [f32], channels: u16) -> usize {
        let channels = usize::from(channels.max(1));
        let written_frames = self
            .source
            .fill(output, channels as u16)
            .min(output.len() / channels);
        let written_samples = written_frames * channels;
        if let Some(state) = self.scene.try_lock() {
            if let Some(gains) = spatial_gains(&state, self.source_id) {
                self.last_gains = gains;
            }
        }
        let [left, right] = self.last_gains;
        for frame in output[..written_samples].chunks_exact_mut(channels) {
            if channels == 1 {
                frame[0] *= (left + right) * 0.5;
                continue;
            }
            frame[0] *= left;
            frame[1] *= right;
            let surround_gain = left.max(right);
            for sample in &mut frame[2..] {
                *sample *= surround_gain;
            }
        }
        written_frames
    }
}

fn allocate_source_id(state: &mut SpatialState) -> Result<SpatialSourceId> {
    let start = state.next_source_id;
    let mut candidate = start;
    loop {
        if !state.source_positions.contains_key(&candidate) {
            state.next_source_id = candidate.wrapping_add(1);
            return Ok(candidate);
        }
        candidate = candidate.wrapping_add(1);
        if candidate == start {
            anyhow::bail!("spatial audio source id space exhausted");
        }
    }
}

fn spatial_gains(state: &SpatialState, id: SpatialSourceId) -> Option<[f32; 2]> {
    let position = state.source_positions.get(&id)?;
    let offset = subtract(*position, state.listener_position);
    let distance = length(offset);
    let direction = normalized(offset, state.listener_forward);
    let right = normalized(
        cross(state.listener_forward, state.listener_up),
        [1.0, 0.0, 0.0],
    );
    let pan = dot(direction, right).clamp(-1.0, 1.0);
    let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
    let attenuation = 1.0 / (1.0 + distance);
    Some([angle.cos() * attenuation, angle.sin() * attenuation])
}

fn finite_position(mut value: [f32; 3]) -> [f32; 3] {
    for component in &mut value {
        if !component.is_finite() {
            *component = 0.0;
        }
    }
    value
}

fn subtract(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(value: [f32; 3]) -> f32 {
    dot(value, value).sqrt()
}

fn normalized(value: [f32; 3], fallback: [f32; 3]) -> [f32; 3] {
    let scale = value
        .iter()
        .map(|component| component.abs())
        .fold(0.0f32, f32::max);
    if !scale.is_finite() || scale <= f32::EPSILON {
        return fallback;
    }
    let scaled = [value[0] / scale, value[1] / scale, value[2] / scale];
    let scaled_length = length(scaled);
    if !scaled_length.is_finite() || scaled_length <= f32::EPSILON {
        return fallback;
    }
    [
        scaled[0] / scaled_length,
        scaled[1] / scaled_length,
        scaled[2] / scaled_length,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BufferSource;

    #[test]
    fn invalid_vectors_are_sanitized_and_orientation_is_orthogonal() {
        let scene = SpatialAudioScene::default();
        scene.set_listener_position([f32::NAN, 2.0, f32::INFINITY]);
        assert_eq!(scene.listener_position(), [0.0, 2.0, 0.0]);

        scene.set_listener_orientation([0.0, 2.0, 0.0], [0.0, 3.0, 0.0]);
        let (forward, up) = scene.listener_orientation();
        assert!(dot(forward, up).abs() < 1e-6);
        assert!((dot(forward, forward) - 1.0).abs() < 1e-6);
        assert!((dot(up, up) - 1.0).abs() < 1e-6);

        scene.set_listener_orientation([f32::MAX, f32::MAX, 0.0], [0.0, f32::MAX, 0.0]);
        let (forward, up) = scene.listener_orientation();
        assert!(dot(forward, up).abs() < 1e-6);
        assert!((dot(forward, forward) - 1.0).abs() < 1e-6);
        assert!((dot(up, up) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn source_ids_do_not_depend_on_track_identity() {
        let scene = SpatialAudioScene::default();
        let first = scene.add_source([1.0, 0.0, 0.0]).unwrap();
        let second = scene.add_source([-1.0, 0.0, 0.0]).unwrap();

        assert_ne!(first, second);
        assert_eq!(scene.source_count(), 2);
        assert!(scene.set_source_position(first, [2.0, 0.0, 0.0]));
        assert_eq!(scene.source_position(first), Some([2.0, 0.0, 0.0]));
        assert!(scene.remove_source(second));
        assert_eq!(scene.source_count(), 1);
    }

    #[test]
    fn spatialized_sources_pan_without_allocating_in_fill() {
        let scene = SpatialAudioScene::default();
        let id = scene.add_source([1.0, 0.0, 0.0]).unwrap();
        let mut source = scene
            .spatialize(id, Box::new(BufferSource::new(vec![1.0, 1.0], 2)))
            .unwrap();
        let mut output = [0.0; 2];

        assert_eq!(source.fill(&mut output, 2), 1);
        assert!(output[1] > output[0]);
        assert!(output.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn scene_rejects_unknown_spatialization() {
        let scene = SpatialAudioScene::default();
        assert!(
            scene
                .spatialize(7, Box::new(BufferSource::new(vec![0.0], 1)))
                .is_err()
        );
    }

    #[test]
    fn removed_sources_keep_their_last_safe_gains() {
        let scene = SpatialAudioScene::default();
        let id = scene.add_source([1.0, 0.0, 0.0]).unwrap();
        let mut source = scene
            .spatialize(id, Box::new(BufferSource::new(vec![1.0, 1.0, 1.0, 1.0], 2)))
            .unwrap();
        let mut first = [0.0; 2];
        let mut second = [0.0; 2];
        assert_eq!(source.fill(&mut first, 2), 1);

        assert!(scene.remove_source(id));
        assert_eq!(source.fill(&mut second, 2), 1);

        assert_eq!(first, second);
    }
}

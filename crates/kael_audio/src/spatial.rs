//! Spatial-audio bookkeeping.

use std::{collections::HashMap, sync::Arc};

use parking_lot::Mutex;

use crate::Track;

/// A lightweight spatial-audio state container.
#[derive(Clone, Debug, Default)]
pub struct SpatialAudioPlayer {
    inner: Arc<Mutex<SpatialState>>,
}

#[derive(Debug)]
struct SpatialState {
    listener_position: [f32; 3],
    listener_forward: [f32; 3],
    listener_up: [f32; 3],
    source_positions: HashMap<u64, [f32; 3]>,
}

impl Default for SpatialState {
    fn default() -> Self {
        Self {
            listener_position: [0.0, 0.0, 0.0],
            listener_forward: [0.0, 0.0, -1.0],
            listener_up: [0.0, 1.0, 0.0],
            source_positions: HashMap::new(),
        }
    }
}

impl SpatialAudioPlayer {
    /// Sets the listener position.
    pub fn set_listener_position(&self, position: [f32; 3]) {
        self.inner.lock().listener_position = finite_position(position);
    }

    /// Sets the listener orientation.
    pub fn set_listener_orientation(&self, forward: [f32; 3], up: [f32; 3]) {
        let forward = normalized(finite_position(forward), [0.0, 0.0, -1.0]);
        let up = finite_position(up);
        let projection = dot(up, forward);
        let orthogonal = [
            up[0] - projection * forward[0],
            up[1] - projection * forward[1],
            up[2] - projection * forward[2],
        ];
        let fallback_up = if forward[1].abs() < 0.9 {
            [0.0, 1.0, 0.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        let up = normalized(orthogonal, fallback_up);
        let mut state = self.inner.lock();
        state.listener_forward = forward;
        state.listener_up = up;
    }

    /// Sets the position for a track source.
    pub fn set_source_position(&self, track: &Track, position: [f32; 3]) {
        self.inner
            .lock()
            .source_positions
            .insert(track.id, finite_position(position));
    }

    /// Returns the current listener position.
    pub fn listener_position(&self) -> [f32; 3] {
        self.inner.lock().listener_position
    }

    /// Returns the current position for a track source.
    pub fn source_position(&self, track: &Track) -> Option<[f32; 3]> {
        self.inner.lock().source_positions.get(&track.id).copied()
    }

    /// Removes the stored spatial position for `track`.
    pub fn remove_source_position(&self, track: &Track) -> bool {
        self.inner
            .lock()
            .source_positions
            .remove(&track.id)
            .is_some()
    }
}

fn finite_position(mut value: [f32; 3]) -> [f32; 3] {
    for component in &mut value {
        if !component.is_finite() {
            *component = 0.0;
        }
    }
    value
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalized(value: [f32; 3], fallback: [f32; 3]) -> [f32; 3] {
    let length = dot(value, value).sqrt();
    if length.is_finite() && length > f32::EPSILON {
        [value[0] / length, value[1] / length, value[2] / length]
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use super::*;
    use crate::AudioSource;

    fn track(id: u64) -> Track {
        Track {
            id,
            source: AudioSource::Memory(Arc::from([])),
            duration: Some(Duration::ZERO),
        }
    }

    #[test]
    fn invalid_vectors_are_sanitized_and_orientation_is_orthogonal() {
        let player = SpatialAudioPlayer::default();
        player.set_listener_position([f32::NAN, 2.0, f32::INFINITY]);
        assert_eq!(player.listener_position(), [0.0, 2.0, 0.0]);

        player.set_listener_orientation([0.0, 2.0, 0.0], [0.0, 3.0, 0.0]);
        let state = player.inner.lock();
        assert!((dot(state.listener_forward, state.listener_up)).abs() < 1e-6);
        assert!((dot(state.listener_forward, state.listener_forward) - 1.0).abs() < 1e-6);
        assert!((dot(state.listener_up, state.listener_up) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn source_positions_can_be_removed() {
        let player = SpatialAudioPlayer::default();
        let track = track(7);
        player.set_source_position(&track, [1.0, 2.0, 3.0]);
        assert!(player.remove_source_position(&track));
        assert_eq!(player.source_position(&track), None);
    }
}
